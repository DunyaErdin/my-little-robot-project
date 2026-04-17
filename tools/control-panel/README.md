# Robot Control Panel

Localhost uzerinde calisan bu arac, robot testlerini, motor komutlarini, sensor snapshot'larini ve telemetry akislarini tek panelde toplar.

Bugun:
- mock robot backend kullanir
- browser Gamepad API ile gamepad axis/button okur
- motor/test/sensor durumlarini localhost API uzerinden yonetir

Yarin:
- ayni HTTP paneli arkasina serial bridge veya Wi-Fi transport eklenebilir

Beklenen adres:
- `http://127.0.0.1:8090`

Bu arac ana ESP32-S3 firmware crate'inden bilerek ayridir; boylece firmware target ayarlari ile localhost panel target'i birbirine karismaz.
