/*
 * Pickup Winder - Uno firmware (serial-controlled executor)
 * ----------------------------------------------------------
 * The Uno does ONLY the hard-real-time work: stepping the motors
 * and counting turns. All settings/recipes come from the desktop
 * app over USB serial at 115200 baud.
 *
 * Command protocol (newline-terminated, ASCII):
 *   SET TURNS <long>     target turn count
 *   SET WIDTH <float>    bobbin width in mm
 *   SET RPM <int>        spindle speed
 *   SET PASS <int>       turns per traverse pass (layering density)
 *   SET JITTER <int>     0-100, scatter-wind randomness
 *   START                begin / resume winding
 *   STOP                 stop and reset turn count to 0
 *   PAUSE                halt, keep turn count
 *   ZERO                 reset turn count to 0
 *   PING                 -> replies "PONG"
 *
 * Reports back (newline-terminated):
 *   TURNS:<n>            streamed during winding
 *   STATE:<RUN|IDLE|PAUSE|DONE>
 *   OK:<echoed command>
 *   ERR:<message>
 *
 * Libraries: AccelStepper
 */

#include <AccelStepper.h>
#include <EEPROM.h>

#define SPIN_STEP 2
#define SPIN_DIR  3
#define TRAV_STEP 4
#define TRAV_DIR  5
#define ENABLE    6

AccelStepper spindle(AccelStepper::DRIVER, SPIN_STEP, SPIN_DIR);
AccelStepper traverse(AccelStepper::DRIVER, TRAV_STEP, TRAV_DIR);

const int   microstep      = 8;
const long  stepsPerTurn   = 200L * microstep;            // 1600 steps/rev
float leadScrewMM          = 8.0;                         // mm travel per traverse-motor rev (calibratable)
float travStepsPerMM       = (200.0 * microstep) / leadScrewMM;

// --- Live settings (set over serial) ---
long  targetTurns = 8000;
float bobbinWidthMM = 17.0;
int   rpm = 420;
int   turnsPerPass = 110;
int   jitter = 0;            // 0-100

// --- Runtime state ---
long  turnsDone = 0;
int   travDir = 1;
float travPos = 0;
bool  running = false;
long  travLimit = 0;
float travPerTurn = 0;

String inBuf = "";

void recalc() {
  travLimit   = bobbinWidthMM * travStepsPerMM;
  travPerTurn = (float)travLimit / max(turnsPerPass, 1);
  // RPM -> steps/sec: rpm/60 * stepsPerTurn
  float sps = (rpm / 60.0) * stepsPerTurn;
  spindle.setMaxSpeed(sps);
  spindle.setAcceleration(sps * 2);
  traverse.setMaxSpeed(travStepsPerMM * 60);
  traverse.setAcceleration(travStepsPerMM * 120);
}

// --- Calibration persistence (EEPROM) ---
// A magic byte marks "valid saved cal"; the float follows it.
const int  EE_MAGIC_ADDR = 0;
const int  EE_LEAD_ADDR  = 1;
const byte EE_MAGIC      = 0x42;

void loadCal() {
  if (EEPROM.read(EE_MAGIC_ADDR) == EE_MAGIC) {
    float v;
    EEPROM.get(EE_LEAD_ADDR, v);
    if (v > 0 && v < 100) {        // sanity guard
      leadScrewMM = v;
      travStepsPerMM = (200.0 * microstep) / leadScrewMM;
    }
  }
}

void saveCal() {
  EEPROM.update(EE_MAGIC_ADDR, EE_MAGIC);
  EEPROM.put(EE_LEAD_ADDR, leadScrewMM);
}

void setup() {
  pinMode(ENABLE, OUTPUT);
  digitalWrite(ENABLE, HIGH);   // disabled (A4988 active-low enable)
  Serial.begin(115200);
  loadCal();                    // restore saved lead-screw calibration
  recalc();
  randomSeed(analogRead(A0));
  Serial.println("STATE:IDLE");
}

void reportState(const char* s) {
  Serial.print("STATE:");
  Serial.println(s);
}

float jitteredTravPerTurn() {
  if (jitter <= 0) return travPerTurn;
  // +/- jitter% random variation to mimic hand scatter-winding
  float j = (random(-jitter, jitter + 1) / 100.0);
  return travPerTurn * (1.0 + j);
}

void handleCommand(String cmd) {
  cmd.trim();
  if (cmd.length() == 0) return;

  if (cmd == "PING") { Serial.println("PONG"); return; }
  if (cmd == "START") {
    running = true;
    digitalWrite(ENABLE, LOW);
    recalc();
    reportState("RUN");
    Serial.print("OK:"); Serial.println(cmd);
    return;
  }
  if (cmd == "STOP") {
    running = false;
    digitalWrite(ENABLE, HIGH);
    turnsDone = 0; travPos = 0; travDir = 1;
    reportState("IDLE");
    Serial.print("OK:"); Serial.println(cmd);
    return;
  }
  if (cmd == "PAUSE") {
    running = false;
    digitalWrite(ENABLE, HIGH);
    reportState("PAUSE");
    Serial.print("OK:"); Serial.println(cmd);
    return;
  }
  if (cmd == "ZERO") {
    turnsDone = 0; travPos = 0; travDir = 1;
    Serial.print("OK:"); Serial.println(cmd);
    return;
  }
  if (cmd.startsWith("SET ")) {
    int sp = cmd.indexOf(' ', 4);
    if (sp < 0) { Serial.println("ERR:bad set"); return; }
    String key = cmd.substring(4, sp);
    String val = cmd.substring(sp + 1);
    if      (key == "TURNS")  targetTurns = val.toInt();
    else if (key == "WIDTH")  bobbinWidthMM = val.toFloat();
    else if (key == "RPM")    rpm = val.toInt();
    else if (key == "PASS")   turnsPerPass = val.toInt();
    else if (key == "JITTER") jitter = constrain(val.toInt(), 0, 100);
    else { Serial.println("ERR:unknown key"); return; }
    recalc();
    Serial.print("OK:"); Serial.println(cmd);
    return;
  }
  if (cmd.startsWith("JOG ")) {
    // Move traverse a signed distance in mm, then stop. For calibration.
    if (running) { Serial.println("ERR:stop winding first"); return; }
    float mm = cmd.substring(4).toFloat();
    long steps = (long)(mm * travStepsPerMM);
    digitalWrite(ENABLE, LOW);
    traverse.setCurrentPosition(0);
    traverse.runToNewPosition(steps);   // blocking move; fine when idle
    digitalWrite(ENABLE, HIGH);
    Serial.print("JOGGED:"); Serial.println(mm);
    Serial.print("OK:"); Serial.println(cmd);
    return;
  }
  if (cmd.startsWith("SETLEADSCREW ")) {
    // Correct the lead-screw travel (mm per rev) live, no re-flash.
    float v = cmd.substring(13).toFloat();
    if (v <= 0) { Serial.println("ERR:bad value"); return; }
    leadScrewMM = v;
    travStepsPerMM = (200.0 * microstep) / leadScrewMM;
    recalc();
    Serial.print("OK:"); Serial.println(cmd);
    return;
  }
  if (cmd == "GETCAL") {
    Serial.print("CAL:leadscrew="); Serial.print(leadScrewMM);
    Serial.print(",stepspermm="); Serial.println(travStepsPerMM);
    return;
  }
  if (cmd == "SAVECAL") {
    saveCal();
    Serial.print("OK:"); Serial.println(cmd);
    return;
  }
  Serial.print("ERR:unknown cmd "); Serial.println(cmd);
}

void readSerial() {
  while (Serial.available()) {
    char c = Serial.read();
    if (c == '\n') { handleCommand(inBuf); inBuf = ""; }
    else if (c != '\r') inBuf += c;
  }
}

void loop() {
  readSerial();

  if (running && turnsDone < targetTurns) {
    spindle.move(stepsPerTurn);
    float thisPass = jitteredTravPerTurn();
    while (spindle.distanceToGo() != 0) {
      spindle.run();
      float frac = 1.0 - (float)abs(spindle.distanceToGo()) / stepsPerTurn;
      traverse.moveTo(travPos + travDir * thisPass * frac);
      traverse.run();
      // stay responsive to STOP/PAUSE mid-turn
      if (Serial.available()) readSerial();
      if (!running) break;
    }
    if (!running) return;
    travPos += travDir * thisPass;
    if (travPos >= travLimit) { travPos = travLimit; travDir = -1; }
    if (travPos <= 0)         { travPos = 0;        travDir =  1; }
    turnsDone++;
    if (turnsDone % 25 == 0) { Serial.print("TURNS:"); Serial.println(turnsDone); }
  }
  else if (running && turnsDone >= targetTurns) {
    running = false;
    digitalWrite(ENABLE, HIGH);
    Serial.print("TURNS:"); Serial.println(turnsDone);
    reportState("DONE");
  }
}
