# Serial Protocol

The app and the Uno communicate over USB serial at **115200 baud**,
newline-terminated ASCII. The Uno is the executor; the app sends
commands and reads back status.

## Commands (app -> Uno)

| Command            | Meaning                                  |
|--------------------|------------------------------------------|
| `SET TURNS <n>`    | Target turn count                        |
| `SET WIDTH <mm>`   | Bobbin width (traverse window)           |
| `SET RPM <n>`      | Spindle speed                            |
| `SET PASS <n>`     | Turns per traverse pass (layer density)  |
| `SET JITTER <0-100>` | Scatter-wind randomness                |
| `START`            | Begin / resume                           |
| `STOP`             | Stop and reset count to 0                |
| `PAUSE`            | Halt, keep count                         |
| `ZERO`             | Reset count to 0                         |
| `PING`             | Health check -> `PONG`                   |
| `JOG <mm>`         | Move traverse a signed distance (idle only) |
| `SETLEADSCREW <mm>`| Set lead-screw travel per rev (live)     |
| `SAVECAL`          | Persist calibration to EEPROM            |
| `GETCAL`           | Report current calibration               |

## Replies (Uno -> app)

| Reply            | Meaning                                   |
|------------------|-------------------------------------------|
| `TURNS:<n>`      | Current turn count (streamed every 25)    |
| `STATE:<s>`      | `RUN` / `IDLE` / `PAUSE` / `DONE`         |
| `OK:<cmd>`       | Command accepted                          |
| `ERR:<msg>`      | Command rejected                          |
| `PONG`           | Response to `PING`                        |
| `CAL:...`        | Current calibration (leadscrew, steps/mm) |
| `JOGGED:<mm>`    | Confirms a jog move completed             |

## Example session
```
>> PING
<< PONG
>> SET TURNS 8000
<< OK:SET TURNS 8000
>> SET JITTER 12
<< OK:SET JITTER 12
>> START
<< STATE:RUN
<< TURNS:25
<< TURNS:50
   ...
<< TURNS:8000
<< STATE:DONE
```
