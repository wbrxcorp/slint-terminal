# slint-terminal

Slint 向けの再利用可能なターミナルエミュレータ。**スタンドアロンのアプリとしても動き、他の Slint アプリケーションにも組み込める**形を目指す。

> このリポジトリはまだスキャフォールド前（設計メモの段階）。本 README は新しい作業セッションが
> コールドスタートできるように、目的・設計方針・未決事項・参考先をまとめたもの。

## 目的と背景

複数の Slint 製 GUI がアプリ内ターミナルを必要とし始めている。最初の需要者は genpack
インストーラ GUI (`genpack-install-gui`)。そこではインストール前後にコマンドライン操作を
させたいが、次の理由で「素朴に VT へ切り替える」方式が使えない:

- 生 KMS/DRM 環境（Wayland コンポジタなし）で動くため、`chvt` で Linux ネイティブ VT へ
  逃がすと起動時に真っ暗になる等 UX が悪い
- ネイティブ VT では日本語表示ができない

そこで **Slint 上に独自のターミナルを描画する**必要がある。これを個別アプリに埋め込むのではなく、
**独立したクレートとして共有**し、将来の Slint 製 GUI でも使い回せるようにするのが本プロジェクト。

## 提供形態（デュアル）

1. **スタンドアロンアプリ**（`[[bin]]`）— ウィンドウ内でシェルを動かす単体ターミナル。
   開発ハーネスも兼ねる（dev 機の Wayland/winit で高速に反復し、同じライブラリが
   インストーラの KMS でも動く。理由は下記「バックエンド非依存」）。
2. **ライブラリ**（`[lib]`）— 他の Slint アプリに Rust API で組み込む。利用側は自分の
   `.slint` に `Image` を1つ置き、毎フレームその `image` プロパティを差し替え、キー/ポインタ
   イベントをターミナルへ転送するだけ。

## アーキテクチャ方針

**フレームワーク非依存コア ＋ slint は薄い feature**、という二層にする。

```
slint-terminal
├─ コア（slint 非依存）
│   ├─ PTY + シェルプロセス          … portable-pty など
│   ├─ VT100/ANSI 状態・文字グリッド   … alacritty-terminal など
│   └─ グリッド → RGBA ピクセルバッファ … フォントラスタライザ（下記・未決）
│       入出力: RGBA(Vec<u8>, w, h) / feed_input / resize / shell 終了通知
└─ feature "slint"（任意・既定で有効）
    └─ RGBA → slint::Image (SharedPixelBuffer) 変換、Slint イベント → 端末入力の橋渡し
```

**なぜコアを slint 非依存にするか（重要）:**
Slint の `Image`/`SharedPixelBuffer` は slint のバージョンに紐づく型で、ライブラリと利用側が
semver 非互換の slint を引くと型が食い違って繋がらない。コアが slint に依存しなければこの結合を
避けられ、将来 slint 以外の GUI でも使える。slint 連携は `feature = "slint"` に隔離し、
ライブラリ側の slint 依存は `"1"` のように緩く指定して利用側と同一版へ解決させる。

**バックエンド非依存:** 端末を「ピクセルバッファ → `Image`」として描くので、Slint の
`backend-winit`（Wayland/X・開発時）でも `backend-linuxkms`（生 KMS・インストーラ実機）でも
同じコードで動く。ここが、dev 機で作って実機へ持っていける根拠。

## 使用予定クレート（候補）

| 役割 | 候補 | 備考 |
|---|---|---|
| PTY・シェル起動 | `portable-pty`（wezterm 製）/ `nix` | portable-pty は高水準で堅牢。nix は低水準 |
| VT 解釈・グリッド | `alacritty-terminal` | Term/Grid/パーサ/PTY イベントループ一式。API 変化が激しいので版固定 |
| フォントラスタライズ | **未決（下記）** | 日本語表示が要件 |
| GUI 連携 | `slint`（feature） | Image/SharedPixelBuffer と入力橋渡しのみ |
| セル幅 | `unicode-width` | CJK の全角=2セル判定（手動レイアウト時） |

## フォントラスタライザの選択肢（未決・要決定）

ターミナルは等幅グリッドだが、**日本語（CJK）表示が必須**、絵文字は任意。ここが設計の分かれ目。

- **fontdue** — pure Rust・軽量・単純なグリフラスタライズ。等幅グリッドと相性が良い。
  ただし複雑な整形やカラー絵文字は非対応、CJK フォントフォールバックは手動。PoC 向き。
- **cosmic-text** — 整形・フォントフォールバック・CJK・（swash 経由で）絵文字まで面倒を見る。
  依存は重いが「ターミナルに日本語」を本格対応するなら安心。共有ライブラリとして育てる路線向き。
- **swash** — cosmic-text の下回り。整形/スケーリングを直接使えるが実装量は増える。
- **ab_glyph / rusttype** — fontdue と同系の pure Rust ラスタライザ。

判断材料: 需要者（genpack-install-gui）の実機フォントは **noto-cjk + noto-emoji のみ**。
まず fontdue で最小 PoC を作り、CJK/絵文字要件が厳しければ cosmic-text へ寄せる、という段階的な
進め方が現実的。ここは着手時に決める。

## 組み込み方法（利用側）

crates.io へ publish しなくても、**cargo の git 依存**で参照できる:

```toml
[dependencies]
slint-terminal = { git = "https://github.com/wbrxcorp/slint-terminal", tag = "v0.1.0" }
# フレームワーク非依存コアだけ使うなら:
#   slint-terminal = { git = "...", tag = "v0.1.0", default-features = false }
```

- `tag`/`rev` で固定し `Cargo.lock` をコミットすれば**再現性が保てる**（genpack の供給網ポリシーに合致）。
- genpack の build.d は `cargo build --release` 時にこの git 依存もネット取得する（ネットワークは使える）。
- **開発中はローカルを指す**（並行開発用）:
  ```toml
  [patch."https://github.com/wbrxcorp/slint-terminal"]
  slint-terminal = { path = "../slint-terminal" }
  ```

## 公開 API（現状）

**コア（`slint_terminal::Terminal`／slint 非依存）:**

```rust
// 生成: cols×rows セル, フォント px, 起動プログラム(None=$SHELL)
Terminal::new(cols: usize, rows: usize, font_px: f32, program: Option<&str>)
    -> Result<Terminal, String>

term.feed_input(&self, bytes: &[u8])          // 端末へ入力バイト列を送る
term.take_dirty(&self) -> bool                // 前回描画以降に変化があったか(取得でクリア)
term.render(&mut self) -> (&[u8], u32, u32)   // 現在のグリッドを RGBA へ。(buf, w, h)
term.resize(&mut self, cols, rows) -> Result<(), String>  // グリッド+PTY を同時リサイズ
term.set_on_exit(&mut self, cb: impl FnMut(u32) + 'static) // シェル終了コールバック登録
term.poll(&mut self)                          // 終了検知して on_exit を1回だけ発火(毎tick呼ぶ)
term.exit_code(&mut self) -> Option<u32>      // 終了コード(ラッチ)。poll を使わない場合用
term.cell_size() -> (usize, usize)            // 1セルのピクセル寸法
term.grid_size() -> (usize, usize)            // 現在の (cols, rows)
term.pixel_size() -> (u32, u32)               // グリッド全体のピクセル寸法
term.cells_for_pixels(px_w, px_h) -> (usize, usize) // ピクセル領域に収まるセル数
```

- スレッド安全性: PTY 入出力は内部ワーカースレッド。グリッドは内部 Mutex で保護。
  `render`/`resize`/`poll`/`exit_code` は UI（描画）スレッドから呼ぶ。`feed_input`/`take_dirty` はどこからでも可。
- `Terminal` を drop するとシェルを kill し、ワーカースレッドも自然終了する（`exit` で終わった後でも、
  実行中に「戻る」で破棄しても安全）。
- `on_exit` は `poll` を呼んだスレッド（Slint ホストでは UI スレッド）で発火する。
  **コールバック内から `Terminal` を drop してはいけない**（`poll` 実行中は借用されている）。
  ホスト側で「戻る」ナビゲーションのフラグを立て、`poll` から戻った後に破棄すること（下記レシピ参照）。

**slint feature（`slint_terminal::slint_glue`）:**

```rust
slint_glue::rgba_to_image(rgba: &[u8], w: u32, h: u32) -> slint::Image
slint_glue::key_to_bytes(text: &str, ctrl: bool, alt: bool) -> Option<Vec<u8>>
//   Slint KeyEvent の text + 修飾フラグ → 端末入力バイト列。
//   名前付き/方向キー→VTシーケンス, Ctrl+英字/記号→C0制御, Alt→ESC前置,
//   修飾キー単独や未対応特殊キーは None（PTY に生コードを流さない）。
```

## genpack-install-gui への組み込み（実装担当向け）

需要者 `genpack-install-gui`（`wbrxcorp/genpack-install-gui`）には既に `Page.terminal` の
プレースホルダがある（`ui/main.slint`、コメントに「PTY + alacritty-terminal + fontdue →
Slint Image、`exit` で戻る」構想）。ここを本クレートで実装する。**slint 版は両者とも緩い `"1"`
指定で同一版に解決される**ので型は繋がる（`backend-winit` + `backend-linuxkms` も共通）。

### 1) 依存追加（`genpack-install-gui/Cargo.toml`）

```toml
slint-terminal = { git = "https://github.com/wbrxcorp/slint-terminal", tag = "v0.1.0" }
# 並行開発中はローカル patch:
# [patch."https://github.com/wbrxcorp/slint-terminal"]
# slint-terminal = { path = "../slint-terminal" }
```

### 2) `.slint`（`Page.terminal` のプレースホルダを置換）

`MainWindow` に次を追加し、端末ページを Image + FocusScope にする:

```slint
in property <image> term-frame;
callback term-key(string, bool, bool);      // text, ctrl, alt
callback term-area-resized(length, length); // 論理サイズ通知（リサイズ対応する場合）

if page == Page.terminal: FocusScope {
    // ページ表示時にフォーカスを取る（forward-focus か init で focus() を）。
    key-pressed(event) => {
        root.term-key(event.text, event.modifiers.control, event.modifiers.alt);
        accept
    }
    term-img := Image {
        source: root.term-frame;
        image-rendering: pixelated;   // 1:1、テキストなので平滑化しない
        width: 100%; height: 100%;
        // サイズが決まったら Rust に論理サイズを渡す（リサイズ追従する場合）
        changed width => { root.term-area-resized(self.width, self.height); }
        changed height => { root.term-area-resized(self.width, self.height); }
    }
}
```

`exit` で戻る挙動は Rust 側 `on_exit` で `page = Page.disk-select` に戻す（プレースホルダの
「Type 'exit' to return」に一致）。

### 3) Rust 側（`src/main.rs`）

端末は**ページに入った時に遅延生成**し、抜ける時に破棄する（root 権限のシェルを常駐させない）。

```rust
use std::cell::RefCell;
use std::rc::Rc;
use slint_terminal::{slint_glue, Terminal};

let term_cell: Rc<RefCell<Option<Terminal>>> = Rc::new(RefCell::new(None));

// 端末ページへ入る導線（既存: メニュー id==1 で page = Page.terminal にしている箇所）で生成。
// 生成サイズは描画領域の物理ピクセルから決める（スケールファクタ考慮、下記「注意」）。
{
    let cell = term_cell.clone();
    let w = window.as_weak();
    window.on_enter_terminal(move || {           // 適宜コールバックを新設 or 既存導線に挿入
        let Some(win) = w.upgrade() else { return };
        let scale = win.window().scale_factor();
        // 端末領域の論理サイズ×scale = 物理ピクセル。まだ未確定なら暫定 80x24 で作り、
        // term-area-resized 受信時に resize する運用でよい。
        let mut t = Terminal::new(80, 24, 16.0, None).expect("terminal");
        let w2 = w.clone();
        t.set_on_exit(move |_code| {
            // ここで Terminal を drop しない。ページを戻すだけ。破棄は tick 側で。
            if let Some(win) = w2.upgrade() { win.set_page(Page::DiskSelect); }
        });
        *cell.borrow_mut() = Some(t);
        win.set_page(Page::Terminal);
    });
}

// キー入力 → PTY
{
    let cell = term_cell.clone();
    window.on_term_key(move |text, ctrl, alt| {
        if let Some(t) = cell.borrow().as_ref() {
            if let Some(bytes) = slint_glue::key_to_bytes(text.as_str(), ctrl, alt) {
                t.feed_input(&bytes);
            }
        }
    });
}

// 描画領域サイズ通知 → グリッドを合わせる（リサイズ追従する場合）
{
    let cell = term_cell.clone();
    let w = window.as_weak();
    window.on_term_area_resized(move |lw, lh| {
        let Some(win) = w.upgrade() else { return };
        let scale = win.window().scale_factor();
        if let Some(t) = cell.borrow_mut().as_mut() {
            let (cols, rows) = t.cells_for_pixels((lw * scale) as u32, (lh * scale) as u32);
            let _ = t.resize(cols, rows);
        }
    });
}

// 毎フレームの駆動: 16ms タイマ（端末ページの間だけ実質動く）
let term_timer = slint::Timer::default();
{
    let cell = term_cell.clone();
    let w = window.as_weak();
    term_timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(16), move || {
        let Some(win) = w.upgrade() else { return };
        let mut guard = cell.borrow_mut();
        let Some(t) = guard.as_mut() else { return };
        t.poll();                                  // ここで on_exit が発火しうる（page が戻る）
        if t.take_dirty() {
            let (rgba, iw, ih) = t.render();
            win.set_term_frame(slint_glue::rgba_to_image(rgba, iw, ih));
        }
        // 端末ページを抜けていたら破棄（on_exit 由来 or 「戻る」ボタン由来）。
        // drop は poll の外＝ここで行うので借用衝突しない。
        if win.get_page() != Page::Terminal {
            *guard = None;
        }
    });
}
```

- `term_timer` はプログラム全体で 1 本持ち回しでよい（`Terminal` が無い間は即 return するので軽い）。
- 「戻る」ボタンは `page = Page.disk-select` にするだけでよい（破棄は tick が拾う）。

### 注意（スケールファクタ ＝ KMS 実機で最重要）

`genpack-install-gui` は生 KMS で**論理/物理を分けてスケールファクタを動的設定**している
（`desired_kms_scale`）。ターミナルは**物理ピクセル解像度で栅化し、論理サイズの Image に流す**と
1:1 で綺麗に出る。したがって:

- グリッドの `cols/rows` は **描画領域の論理サイズ × `window().scale_factor()`（＝物理px）** から
  `cells_for_pixels()` で求める。
- Image は論理サイズ（`width/height: 100%`）で置き、`image-rendering: pixelated`。
  Slint がソース（物理px）を要素（論理）へ 1:1 マップしてフレームバッファに描く。
- スケール変更（`ScaleFactorChanged`）時は `term-area-resized` が再発火するので resize が追従する。
- まず固定 80×24 で通し、リサイズ追従は後追いでも可。ただし KMS 実機は 720p〜4K まで幅があるので
  最終的には物理px 由来のサイズ決定を入れること。

### スレッドモデルの整合

本クレートは PTY I/O を内部ワーカースレッドで回すので、`genpack-install-gui` の
`run_busy`/`invoke_from_event_loop` パターンとは独立して動く。端末描画は上記 16ms タイマ
（UI スレッド）だけで完結し、busy オーバーレイとも干渉しない。`Terminal` 生成/破棄・
`render`/`resize`/`poll` は UI スレッドから呼ぶこと。

### 環境まわり

- 起動プログラムは `Terminal::new(.., program)` で指定。`None` で `$SHELL`（無ければ `/bin/sh`）。
  インストーラで特定シェルを使いたければ `Some("/bin/bash")` 等を渡す。
- コアが `TERM=xterm-256color` を設定し、cwd はホストプロセスから継承する。
- フォントは fontconfig の `monospace`（実機は `Noto Sans Mono CJK JP`）を自動解決。絵文字は非対応。

## 設計メモ・落とし穴

- **スレッド境界**: PTY 読み取りはワーカースレッド。UI 反映は `slint::invoke_from_event_loop`
  でメインスレッドへ戻す。需要者 genpack-install-gui の `run_busy`/`invoke_from_event_loop`
  パターンが参考になる。
- **リサイズ**: ウィンドウ/セル数変更時に PTY を `TIOCSWINSZ` でリサイズし、端末状態も追随。
- **入力マッピング**: Slint の `KeyEvent` → 端末入力バイト列（特殊キー・修飾キー・矢印/Fキー等）。
  ここが一番地味に手間がかかる。
- **シェル終了通知**: `Terminal::set_on_exit` で登録し `Terminal::poll` を毎tick呼ぶと、シェル
  exit 時にホストへ通知（genpack の「exit で GUI 画面に戻る」用）。実装済み。コールバック内から
  `Terminal` を drop しない点に注意（上記「組み込み」参照）。
- **slint 版結合**: 上記のとおり slint は feature に隔離、依存は緩く。

## 参考・開発コンテキスト（コールドスタート時にまず見る）

- **需要者（最初の利用側）**: `~/projects/genpack-install-gui/`
  （GitHub: `wbrxcorp/genpack-install-gui`）。特に:
  - `README.md` のターミナル画面の節（PTY + alacritty-terminal + フォントラスタライザ →
    Slint `Image` という構想、`chvt` を避ける理由）
  - 非同期・UIスレッドの扱い（ワーカースレッド + `invoke_from_event_loop`）
  - 実機フォント制約（noto-cjk + noto-emoji のみ）、生 KMS 前提
  - `ui/main.slint` の画面構成（ターミナルは現状プレースホルダ）
- **Slint 公式ドキュメント**: https://slint.dev/docs （`Image`/`SharedPixelBuffer`、
  バックエンド、キーイベント）

## ステータス

**最小 PoC 実装済み・実機動作確認済み**（fontdue ベース）。
スタンドアロンアプリで「PTY → alacritty-terminal → fontdue ラスタライズ → Slint `Image`」の
一巡が通り、シェル入力・日本語（CJK, 全角=2セル）表示・Ctrl 系キー（Ctrl+C 等）・方向キーが動作する。
シェル終了コールバック（`set_on_exit`/`poll`）も実装済みで、`exit` でホストへ通知できる。

構成:

```
src/lib.rs        コア公開 API: Terminal（slint 非依存）
src/pty.rs        portable-pty でシェル起動 + 読み書きスレッド
src/terminal.rs   alacritty_terminal Term ラッパ（parser/EventListener/Dimensions/resize）
src/font.rs       fontdue: fc-match で monospace(.ttc index) 解決 + グリフキャッシュ
src/render.rs     グリッド → RGBA（16/256 色 + RGB 直指定の解決）
src/slint_glue.rs feature "slint": RGBA→slint::Image, KeyEvent(+修飾)→入力バイト列
src/bin/slint-terminal.rs  スタンドアロン（required-features = ["slint"]）
ui/main.slint     Image 1 枚 + FocusScope
```

- フォントは `Noto Sans Mono CJK JP`（fontconfig 解決）1 枚で ASCII+日本語を賄う。
- `cargo build`（既定・slint 有効）と `cargo build --no-default-features --lib`（コア単独・slint 非依存）が
  それぞれ通ることを確認済み ＝ 二層分離が機能している。

### 既知の割り切り（PoC 時点）

- **絵文字は非対応**（豆腐/×表示）。fontdue はカラー絵文字非対応で、実機フォントにも絵文字グリフが無いため。
  必要になったら cosmic-text/swash のカラー経路へ寄せる（本 README「フォントラスタライザの選択肢」参照）。
- 太字/イタリック/下線などの装飾は未処理（色反転 INVERSE のみ対応）。
- カーソルはブロック固定（形状・点滅なし）。
- HiDPI（スケール係数 > 1）では Image が 1:1 描画にならない場合がある。
- 修飾キー: Ctrl+英字・記号 → C0 制御、Alt → ESC 前置、方向/Home/End/PgUp/PgDn/Delete まで対応。
  F キー等の残りは今後。

### 次の候補

需要者 `genpack-install-gui` への実組み込み（上記レシピに沿って `Page.terminal` を実装）と
KMS 実機での 1:1 描画・スケール追従の検証。装飾（太字/下線）・カーソル形状・F キー・
絵文字（cosmic-text 経路）は必要に応じて追加。

## ライセンス

未定（リポジトリ本体）。**注意**: `slint` feature を有効にすると Slint の三択ライセンス
（GPLv3 / Royalty-Free / 商用）の考慮が利用側に及ぶ（"Made with Slint" 帰属か GPLv3）。
slint 非依存のコアだけを使う経路にはこの影響はない。
