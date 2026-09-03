# ch32v305-DAC

CH32V305 で USB Audio Class 2.0 の再生ストリームを受け取り、I2S で 16-bit stereo / 48 kHz を出力する Rust + Embassy の最小構成サンプルです。

## 構成

- USB: OTG_FS, UAC2 speaker sink
- Audio format: PCM, 16-bit, stereo, 48 kHz 固定
- I2S: SPI2 を I2S master transmit として使用
- 想定ピン:
  - USB DM: `PA11`
  - USB DP: `PA12`
  - I2S WS/LRCK: `PB12`
  - I2S CK/BCLK: `PB13`
  - I2S SD: `PB15`

## ビルド

```bash
cargo build --release
```

別パッケージの CH32V305 を使う場合は feature を切り替えてください。

```bash
cargo build --release --no-default-features --features chip-ch32v305gbu6
```

## 注意

- `ch32-hal` には UAC2 の完成済みクラスがないため、このサンプルは `embassy-usb` のカスタム記述子で最小限の UAC2 sink を実装しています。
- サンプルレートは 48 kHz 固定です。
- I2S の分周は 144 MHz 系クロック前提の簡易計算です。実ボードのクロック条件や接続先 DAC/codec に応じて調整が必要です。
- MCLK は出していません。必要なら SPI2_MCK の有効化とピン設定を追加してください。
