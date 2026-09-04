# ch32v305-DAC

CH32V305 で USB Audio Class 2.0 の再生ストリームを受け取り、I2S で 16-bit / 24-bit stereo を出力する Rust + Embassy の最小構成サンプルです。

## 構成

- USB: OTG_FS, UAC2 speaker sink
- 同期方式: 非同期 OUT + 明示的フィードバックエンドポイント
- Audio format:
  - Alt 1: PCM, 16-bit, stereo, 44.1 / 48 / 88.2 / 96 kHz
  - Alt 2: PCM, 24-bit packed, stereo, 44.1 / 48 kHz
- I2S: SPI2 を I2S master transmit として使用
- バッファリング: USB と I2S の間に 32 ms 分のソフトウェア FIFO を配置
- 送信方式: SPI2 TX は DMA1_CH5 の circular DMA で連続出力
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
- ホスト側の転送レート追従用に、現在選択中のサンプルレートに対応した明示的フィードバックエンドポイントを返します。
- `ch32-hal` の USBFS 実装は 512 byte を超える等時 OUT バッファを扱えないため、このサンプルで安定動作の対象にしているのは 16-bit で 96 kHz まで、24-bit で 48 kHz までです。
- USB から届いた PCM は一度サンプル FIFO へ貯め、I2S 側は DMA リングバッファへ先回りで供給しています。
- FIFO が不足した区間は無音で埋め、FIFO が溢れそうなときは古いサンプルを捨てて遅延の増大を抑えています。
- I2S の分周は 144 MHz 系クロック前提の簡易計算です。実ボードのクロック条件や接続先 DAC/codec に応じて調整が必要です。
- MCLK は出していません。必要なら SPI2_MCK の有効化とピン設定を追加してください。
