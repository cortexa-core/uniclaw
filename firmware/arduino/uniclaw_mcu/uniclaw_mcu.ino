// UniClaw MCU — Reference Arduino firmware for serial bridge
//
// Protocol: [0xAA] [LEN:u16-LE] [SEQ:u8] [TYPE:u8] [PAYLOAD...] [CRC8]
// CRC8 = XOR of all bytes from LEN through PAYLOAD.
// Baud: 115200

#include <Servo.h>

// --- Wire constants ---
#define START_BYTE     0xAA
#define CMD_SERVO_SET       0x01
#define CMD_MOTOR_SET       0x02
#define CMD_LED_SET         0x03
#define CMD_LED_PATTERN     0x04
#define CMD_SENSOR_REQUEST  0x05
#define CMD_PING            0x06
#define CMD_E_STOP          0x07
#define CMD_STATUS_REQUEST  0x08
#define CMD_HEARTBEAT       0x09

#define RESP_ACK         0x81
#define RESP_SENSOR_DATA 0x82
#define RESP_STATUS      0x83
#define RESP_ERROR       0x84
#define RESP_PONG        0x85

#define MAX_PAYLOAD 32
#define HEARTBEAT_TIMEOUT_MS 500

// --- State ---
static uint8_t rxBuf[64];
static uint8_t rxLen = 0;
static unsigned long lastHeartbeat = 0;
static bool watchdogTripped = false;

// Optional servo on pin 9
static Servo myServo;
static bool servoAttached = false;

// --- CRC ---
static uint8_t crc8(const uint8_t *data, uint16_t len) {
    uint8_t crc = 0;
    for (uint16_t i = 0; i < len; i++) crc ^= data[i];
    return crc;
}

// --- Send a response frame ---
static void sendFrame(uint8_t seq, uint8_t type, const uint8_t *payload, uint16_t plen) {
    uint16_t innerLen = 2 + plen; // seq + type + payload
    uint8_t lenLo = innerLen & 0xFF;
    uint8_t lenHi = (innerLen >> 8) & 0xFF;

    // Compute CRC over LEN + SEQ + TYPE + PAYLOAD
    uint8_t crc = lenLo ^ lenHi ^ seq ^ type;
    for (uint16_t i = 0; i < plen; i++) crc ^= payload[i];

    Serial.write(START_BYTE);
    Serial.write(lenLo);
    Serial.write(lenHi);
    Serial.write(seq);
    Serial.write(type);
    Serial.write(payload, plen);
    Serial.write(crc);
}

static void sendAck(uint8_t seq, uint8_t cmd) {
    uint8_t payload[2] = { seq, cmd };
    sendFrame(seq, RESP_ACK, payload, 2);
}

static void sendPong(uint8_t seq) {
    sendFrame(seq, RESP_PONG, NULL, 0);
}

static void sendStatus(uint8_t seq) {
    uint8_t payload[2] = { 100, 0x00 }; // battery=100%, error_flags=0
    sendFrame(seq, RESP_STATUS, payload, 2);
}

static void sendSensorData(uint8_t seq, uint8_t id, uint8_t stype, int32_t value) {
    uint8_t payload[6];
    payload[0] = id;
    payload[1] = stype;
    payload[2] = (uint8_t)(value & 0xFF);
    payload[3] = (uint8_t)((value >> 8) & 0xFF);
    payload[4] = (uint8_t)((value >> 16) & 0xFF);
    payload[5] = (uint8_t)((value >> 24) & 0xFF);
    sendFrame(seq, RESP_SENSOR_DATA, payload, 6);
}

// --- Process a decoded frame ---
static void handleFrame(uint8_t seq, uint8_t type, const uint8_t *payload, uint16_t plen) {
    switch (type) {
        case CMD_PING:
            sendPong(seq);
            break;

        case CMD_HEARTBEAT:
            lastHeartbeat = millis();
            watchdogTripped = false;
            sendAck(seq, CMD_HEARTBEAT);
            break;

        case CMD_STATUS_REQUEST:
            sendStatus(seq);
            break;

        case CMD_SERVO_SET:
            if (plen >= 5) {
                uint8_t id = payload[0];
                uint16_t angle = payload[1] | ((uint16_t)payload[2] << 8);
                uint16_t spd = payload[3] | ((uint16_t)payload[4] << 8);
                // Attach servo on first use
                if (!servoAttached) {
                    myServo.attach(9);
                    servoAttached = true;
                }
                myServo.write(constrain(angle, 0, 180));
                (void)spd; // speed not used by standard Servo lib
                sendAck(seq, CMD_SERVO_SET);
            }
            break;

        case CMD_MOTOR_SET:
            if (plen >= 5) {
                sendAck(seq, CMD_MOTOR_SET);
            }
            break;

        case CMD_LED_SET:
            if (plen >= 4) {
                sendAck(seq, CMD_LED_SET);
            }
            break;

        case CMD_LED_PATTERN:
            if (plen >= 2) {
                sendAck(seq, CMD_LED_PATTERN);
            }
            break;

        case CMD_SENSOR_REQUEST:
            if (plen >= 1) {
                uint8_t id = payload[0];
                // Return a dummy analog reading as raw type (type=0xFF)
                int32_t val = analogRead(A0);
                sendSensorData(seq, id, 0xFF, val);
            }
            break;

        case CMD_E_STOP:
            // Detach servo, set all outputs low
            if (servoAttached) {
                myServo.detach();
                servoAttached = false;
            }
            digitalWrite(LED_BUILTIN, LOW);
            sendAck(seq, CMD_E_STOP);
            break;

        default:
            break;
    }
}

// --- Try to parse a frame from rxBuf ---
static bool tryParse() {
    if (rxLen < 6) return false;
    if (rxBuf[0] != START_BYTE) {
        // Discard leading garbage — scan for next START_BYTE
        uint8_t shift = 1;
        while (shift < rxLen && rxBuf[shift] != START_BYTE) shift++;
        memmove(rxBuf, rxBuf + shift, rxLen - shift);
        rxLen -= shift;
        return false;
    }

    uint16_t innerLen = rxBuf[1] | ((uint16_t)rxBuf[2] << 8);
    uint16_t total = 1 + 2 + innerLen + 1;
    if (total > sizeof(rxBuf)) { rxLen = 0; return false; } // too large
    if (rxLen < total) return false; // incomplete

    // Verify CRC: XOR of bytes [1..3+innerLen)
    uint8_t expected = crc8(rxBuf + 1, 2 + innerLen);
    if (rxBuf[total - 1] != expected) {
        // Bad CRC — discard first byte and retry
        memmove(rxBuf, rxBuf + 1, rxLen - 1);
        rxLen--;
        return false;
    }

    uint8_t seq = rxBuf[3];
    uint8_t type = rxBuf[4];
    handleFrame(seq, type, rxBuf + 5, innerLen - 2);

    // Remove consumed bytes
    memmove(rxBuf, rxBuf + total, rxLen - total);
    rxLen -= total;
    return true;
}

// --- Arduino setup/loop ---
void setup() {
    Serial.begin(115200);
    pinMode(LED_BUILTIN, OUTPUT);
    digitalWrite(LED_BUILTIN, LOW);
    lastHeartbeat = millis();
}

void loop() {
    // Read available bytes
    while (Serial.available() && rxLen < sizeof(rxBuf)) {
        rxBuf[rxLen++] = Serial.read();
    }

    // Try to parse frames
    while (tryParse()) { /* process all complete frames */ }

    // Watchdog: blink LED 13 if no heartbeat for HEARTBEAT_TIMEOUT_MS
    if (millis() - lastHeartbeat > HEARTBEAT_TIMEOUT_MS) {
        if (!watchdogTripped) {
            watchdogTripped = true;
        }
        // Blink at ~2 Hz
        digitalWrite(LED_BUILTIN, (millis() / 250) % 2 ? HIGH : LOW);
    } else {
        digitalWrite(LED_BUILTIN, HIGH); // solid = connected
    }
}
