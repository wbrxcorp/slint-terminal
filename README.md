# slint-terminal

Slint 向けの再利用可能なターミナルエミュレータ。**スタンドアロンのアプリとしても、他の
Slint アプリへの組み込み部品としても**動く。

PTY で本物のシェルを起動し、VT100/ANSI を解釈して等幅グリッドを RGBA ピクセルバッファへ
ラスタライズし、それを `slint::Image` として表示する。生 KMS/DRM（Wayland コンポジタなし）でも
winit（Wayland/X）でも同じコードで動くため、開発機で作ってそのまま実機へ持っていける。

## 特徴

- **本物の PTY + シェル**（[portable-pty]）— インタラクティブシェルがそのまま動く
- **VT100/ANSI 解釈**（[alacritty_terminal]）— 16/256 色・RGB 直指定・反転に対応
- **日本語（CJK）表示** — 全角は 2 セル。[fontdue] でラスタライズし、フォントは fontconfig で
  等幅フォントを自動解決（実機の `Noto Sans Mono CJK JP` 1 枚で ASCII＋日本語を賄う）
- **二形態** — スタンドアロンバイナリと、Rust API で組み込むライブラリ
- **フレームワーク非依存コア** — `default-features = false` で slint 非依存のコアだけ使える
- **バックエンド非依存** — Slint の `backend-winit` でも `backend-linuxkms` でも動く
- **キー入力マッピング** — Ctrl/Alt 修飾、方向・Home/End・PageUp/Down・Delete などの VT シーケンス化
- **シェル終了コールバック** — `exit` でホストへ通知（GUI 画面へ戻る等に使う）

[portable-pty]: https://crates.io/crates/portable-pty
[alacritty_terminal]: https://crates.io/crates/alacritty_terminal
[fontdue]: https://crates.io/crates/fontdue

## アーキテクチャ

**フレームワーク非依存コア ＋ 薄い slint feature** の二層構成。

```
slint-terminal
├─ コア（slint 非依存）
│   ├─ PTY + シェルプロセス           … portable-pty
│   ├─ VT100/ANSI 状態・文字グリッド    … alacritty_terminal
│   └─ グリッド → RGBA ピクセルバッファ  … fontdue（グリフキャッシュ）
│      公開型 Terminal: render() が (&[u8], w, h) を返す / feed_input / resize / on_exit
└─ feature "slint"（既定で有効）
    └─ RGBA → slint::Image 変換、Slint KeyEvent → 端末入力バイト列の橋渡し
```

**コアを slint 非依存にしている理由:** `slint::Image` / `SharedPixelBuffer` は slint の
バージョンに紐づく型で、ライブラリと利用側が semver 非互換の slint を引くと型が食い違って
繋がらなくなる。コアが slint に依存しなければこの結合を避けられ、将来 slint 以外の GUI でも
使える。slint 連携は `feature = "slint"` に隔離し、依存を `"1"` と緩く指定して利用側と同一版へ
解決させている。

## スタンドアロンで動かす

```sh
cargo run --release
```

ウィンドウ内でログインシェル（`$SHELL`）が動く。通常のキー入力・日本語・Ctrl 系キー
（Ctrl+C 等）・方向キーが使える。シェルで `exit` するか Ctrl+D でウィンドウが閉じる。

## ライブラリとして組み込む

crates.io へ publish せずとも **cargo の git 依存**で参照できる。`tag`/`rev` で固定し
`Cargo.lock` をコミットすれば再現性が保てる。

```toml
[dependencies]
slint-terminal = { git = "https://github.com/wbrxcorp/slint-terminal", tag = "v0.1.0" }
# slint 非依存のコアだけ使うなら:
#   slint-terminal = { git = "...", tag = "v0.1.0", default-features = false }

# 並行開発中はローカルを指す:
# [patch."https://github.com/wbrxcorp/slint-terminal"]
# slint-terminal = { path = "../slint-terminal" }
```

利用側がやることは「`.slint` に `Image` を 1 枚置き、毎フレームその `image` プロパティを
差し替え、キーイベントを端末へ転送する」だけ。

### `.slint`

```slint
in property <image> term-frame;
callback term-key(string, bool, bool); // text, ctrl, alt

FocusScope {
    key-pressed(event) => {
        root.term-key(event.text, event.modifiers.control, event.modifiers.alt);
        accept
    }
    Image {
        source: root.term-frame;
        image-rendering: pixelated; // テキストなので平滑化しない
        width: 100%;
        height: 100%;
    }
}
```

### Rust（描画ループとイベント配線）

端末はページ表示時に遅延生成し、離脱時に破棄すると、シェルを常駐させずに済む。

```rust
use std::cell::RefCell;
use std::rc::Rc;
use slint_terminal::{slint_glue, Terminal};

let term: Rc<RefCell<Option<Terminal>>> = Rc::new(RefCell::new(None));

// 生成（描画領域の物理ピクセルからセル数を決めるのが望ましい。下記「KMS…」参照）
{
    let mut t = Terminal::new(80, 24, 16.0, None)?; // cols, rows, font_px, program(None=$SHELL)
    let w = window.as_weak();
    t.set_on_exit(move |_code| {
        // ここで Terminal を drop しないこと（poll 実行中は借用されている）。
        // ページを戻す等のフラグ操作だけ行い、破棄は描画ループ側で。
        if let Some(win) = w.upgrade() { win.set_page(Page::Home); }
    });
    *term.borrow_mut() = Some(t);
}

// キー入力 → 端末
{
    let term = term.clone();
    window.on_term_key(move |text, ctrl, alt| {
        if let Some(t) = term.borrow().as_ref() {
            if let Some(bytes) = slint_glue::key_to_bytes(text.as_str(), ctrl, alt) {
                t.feed_input(&bytes);
            }
        }
    });
}

// 描画ループ（タイマ。差分があるフレームだけ Image を差し替える）
let timer = slint::Timer::default();
{
    let term = term.clone();
    let w = window.as_weak();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(33), move || {
        let Some(win) = w.upgrade() else { return };
        let mut guard = term.borrow_mut();
        let Some(t) = guard.as_mut() else { return };

        t.poll(); // シェル終了を検知し on_exit を1回だけ発火

        if t.take_dirty() {
            let (rgba, iw, ih) = t.render();
            win.set_term_frame(slint_glue::rgba_to_image(rgba, iw, ih));
        }

        // 離脱していれば破棄（drop は poll の外なので借用衝突しない）
        if win.get_page() != Page::Terminal {
            *guard = None;
        }
    });
}
```

### KMS 実機でのスケールファクタ（重要）

生 KMS では論理座標と物理ピクセルを分けてスケールファクタを設定する構成が多い。ターミナルは
**物理ピクセル解像度でラスタライズし、論理サイズの `Image` に流す**と 1:1 で綺麗に出る。

- グリッドの `cols/rows` は **描画領域の論理サイズ × `window().scale_factor()`（＝物理px）** から
  `cells_for_pixels()` で求め、`resize()` する。
- `Image` は論理サイズ（`width/height: 100%`）で置き `image-rendering: pixelated`。Slint が
  ソース（物理px）を要素（論理）へ 1:1 マップして描く。

まずは固定 80×24 で通し、リサイズ追従は後追いでも構わない。

### 描画性能について

`render()` 自体は軽い（20px・80×24・release で 1 フレームあたり約 0.6ms）。連続出力時のコストは
主に Slint 側のテクスチャアップロード＋コンポジットなので、**描画ループの間隔で present 頻度を
上限する**（上例の 33ms ＝ 30fps）のが最も効く。静止時は `take_dirty()` により再描画しない。

## 公開 API

### コア（`slint_terminal::Terminal`／slint 非依存）

```rust
// 生成: cols×rows セル, フォント px, 起動プログラム(None=$SHELL)
Terminal::new(cols: usize, rows: usize, font_px: f32, program: Option<&str>)
    -> Result<Terminal, String>

fn feed_input(&self, bytes: &[u8])            // 端末へ入力バイト列を送る
fn take_dirty(&self) -> bool                  // 前回描画以降に変化があったか（取得でクリア）
fn render(&mut self) -> (&[u8], u32, u32)     // 現在のグリッドを RGBA へ。(buf, width, height)
fn resize(&mut self, cols, rows) -> Result<(), String> // グリッド＋PTY を同時リサイズ
fn set_on_exit(&mut self, cb: impl FnMut(u32) + 'static) // シェル終了コールバック登録
fn poll(&mut self)                            // 終了を検知し on_exit を1回だけ発火（毎tick呼ぶ）
fn exit_code(&mut self) -> Option<u32>        // 終了コード（ラッチ）。poll を使わない場合用
fn cell_size(&self) -> (usize, usize)         // 1セルのピクセル寸法
fn grid_size(&self) -> (usize, usize)         // 現在の (cols, rows)
fn pixel_size(&self) -> (u32, u32)            // グリッド全体のピクセル寸法
fn cells_for_pixels(&self, px_w, px_h) -> (usize, usize) // ピクセル領域に収まるセル数
```

- **スレッド安全性**: PTY 入出力は内部ワーカースレッドで回り、グリッドは内部 Mutex で保護される。
  `render`/`resize`/`poll`/`exit_code` は UI（描画）スレッドから呼ぶこと。`feed_input`/`take_dirty`
  はどのスレッドからでも呼べる。
- **後始末**: `Terminal` を drop するとシェルを kill し、ワーカースレッドも自然終了する
  （`exit` 後でも、実行中に破棄しても安全）。
- **`on_exit` の注意**: コールバックは `poll` を呼んだスレッド（Slint ホストでは UI スレッド）で
  発火する。**コールバック内から `Terminal` を drop してはいけない**（`poll` 実行中は借用中）。
  破棄は `poll` から戻った後に行う。

### slint feature（`slint_terminal::slint_glue`）

```rust
fn rgba_to_image(rgba: &[u8], w: u32, h: u32) -> slint::Image
fn key_to_bytes(text: &str, ctrl: bool, alt: bool) -> Option<Vec<u8>>
//   Slint KeyEvent の text＋修飾フラグ → 端末入力バイト列。
//   名前付き/方向キー→VTシーケンス、Ctrl+英字/記号→C0制御、Alt→ESC前置。
//   修飾キー単独や未対応の特殊キーは None（PTY に生コードを流さない）。
```

## フォント

fontconfig の `monospace`（`fc-match` 経由、TrueType コレクションの index も解決）を実行時に
読み込む。実機では `Noto Sans Mono CJK JP` に解決され、ASCII と日本語を単一フォントで賄う。
カラー絵文字は非対応（下記「制限事項」）。

## Feature flags

| feature | 既定 | 内容 |
|---|---|---|
| `slint` | on | `slint_glue`（RGBA→`slint::Image`、キーマッピング）とスタンドアロンバイナリを有効化 |

slint 非依存のコアだけ使う場合は `default-features = false`。

## ビルド

```sh
cargo build                                  # 既定（slint 有効）＋スタンドアロンバイナリ
cargo build --no-default-features --lib      # コア単独（slint 非依存）
cargo run --release --example bench_render   # 描画パスの簡易ベンチ（ディスプレイ不要）
```

## 制限事項

- **カラー絵文字は非対応**（豆腐/×表示）。fontdue はカラー絵文字非対応で、実機フォントにも
  絵文字グリフが無いため。必要になれば cosmic-text/swash のカラー経路への差し替えを検討。
- 太字・イタリック・下線などの装飾は未処理（色の反転 INVERSE のみ対応）。
- カーソルはブロック固定（形状・点滅なし）。
- キー入力は Ctrl+英字/記号・Alt・方向/Home/End/PageUp/PageDown/Delete まで。F キー等は今後。

## ロードマップ

- 装飾（太字/下線）、カーソル形状、F キーなど入力マッピングの拡充
- 実機での描画性能プロファイル（render 時間 vs present 時間の分離、KMS レンダラの比較）
- 必要に応じて cosmic-text 経路によるカラー絵文字対応

## ライセンス

本プロジェクトは以下のいずれかを利用者が選択できるデュアルライセンス:

- MIT license（[LICENSE-MIT](LICENSE-MIT)）
- Apache License 2.0（[LICENSE-APACHE](LICENSE-APACHE)）

`SPDX: MIT OR Apache-2.0`

**Slint 依存についての注意:** `slint` feature を有効にすると、最終成果物には Slint 本体が
リンクされ、その配布は **Slint の三択ライセンス（GPLv3 / "Made with Slint" 帰属の Royalty-Free /
商用）のいずれか**に従う必要がある（これは配布者の義務であり、本プロジェクト自身の MIT/Apache-2.0
とは独立・両立する）。`default-features = false` の slint 非依存コアだけを使う経路には Slint 由来の
制約はかからない。

### Contribution

明示的に別段の意思表示をしない限り、あなたが本プロジェクトへの取り込みを意図して提出した貢献は、
Apache-2.0 license の定義に従い、追加の条項なしに上記のとおりデュアルライセンスされるものとします。
