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

## 設計メモ・落とし穴

- **スレッド境界**: PTY 読み取りはワーカースレッド。UI 反映は `slint::invoke_from_event_loop`
  でメインスレッドへ戻す。需要者 genpack-install-gui の `run_busy`/`invoke_from_event_loop`
  パターンが参考になる。
- **リサイズ**: ウィンドウ/セル数変更時に PTY を `TIOCSWINSZ` でリサイズし、端末状態も追随。
- **入力マッピング**: Slint の `KeyEvent` → 端末入力バイト列（特殊キー・修飾キー・矢印/Fキー等）。
  ここが一番地味に手間がかかる。
- **シェル終了通知**: シェルが exit したらホスト側へコールバックで通知（genpack では
  「exit で GUI 画面に戻る」を実現するため）。
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

lib/bin と feature の整理（現状 lib はほぼ整理済み）、シェル終了コールバックの公開 API 化、
需要者 `genpack-install-gui` への git 依存での組み込み検証。

## ライセンス

未定（リポジトリ本体）。**注意**: `slint` feature を有効にすると Slint の三択ライセンス
（GPLv3 / Royalty-Free / 商用）の考慮が利用側に及ぶ（"Made with Slint" 帰属か GPLv3）。
slint 非依存のコアだけを使う経路にはこの影響はない。
