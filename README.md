# ch32v305-DAC

CH32V305 で USB Audio Class 2.0 の再生ストリームを受け取り、USBHS から I2S へ 32-bit stereo / 192 kHz を出力する Rust + Embassy の最小構成サンプルです。

## 構成

- USB: USBHS, UAC2 speaker sink
- 同期方式: 非同期 OUT + 明示的フィードバックエンドポイント
- Audio format: PCM, 32-bit, stereo, 192 kHz 固定
- I2S: SPI2 を I2S master transmit として使用
- バッファリング: USB と I2S の間に約 8 ms 分のソフトウェア FIFO を配置
- 送信方式: SPI2 TX は DMA1_CH5 の circular DMA で連続出力
- 想定ピン:
  - USBHS DM: `PB6`
  - USBHS DP: `PB7`
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
- ホスト側の転送レート追従用に、192 kHz 固定の明示的フィードバックエンドポイントを返します。
- サンプルレートは 192 kHz 固定です。
- USB から届いた PCM は一度サンプル FIFO へ貯め、I2S 側は DMA リングバッファへ先回りで供給しています。
- FIFO が不足した区間は無音で埋め、FIFO が溢れそうなときは古いサンプルを捨てて遅延の増大を抑えています。
- I2S の分周は 144 MHz 系クロック前提で 32-bit stereo / 192 kHz のビットクロックに合わせた簡易計算です。実ボードのクロック条件や接続先 DAC/codec に応じて調整が必要です。
- MCLK は出していません。必要なら SPI2_MCK の有効化とピン設定を追加してください。
