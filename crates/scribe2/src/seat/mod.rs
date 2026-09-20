//! `seat` subcommand の面（設計 docs/design/seat-autonomy.md §3・seat-state.md）: 開発 session の
//! context 使用量を読む [`meter`]、tmux pane へ 1 行を送る [`inject`]、席の状態を hook の打刻で
//! typed に持つ [`state`]。
//!
//! **env も HOME も読まない**（憲法 C2.2）。pane は `--target`（または明示された
//! `--capture-file`）だけを見て、transcript は `--transcript` で明示された file だけを
//! 読む。tmux は外部 process の呼出しで、crate 依存は増えない（憲法 A3 非該当）。
//! **pane の字面は席の busy / idle の判定入力にしない**（憲法 C3.3・ADR-0015）: 字面を読むのは
//! statusline の数値（[`meter`]）と注入の送達確認（[`inject`]）だけである。
//!
//! 出力は行を組んで返すだけで、stdout / stderr へは bin 側の `emit` / `emit_err` が
//! 書く（憲法 C2）。

pub mod brief;
pub mod cli;
pub mod cycle;
pub mod inject;
pub mod ledger;
pub mod recent;
pub mod role;
pub mod state;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 開発 session の入力欄を指す prompt の字。
///
/// pane の読みはこの字を **anchor** に使う（statusline は最後の prompt 行より下に
/// 描かれ、打ちかけの text はその右に在る）。
pub const PROMPT: char = '❯';

/// tmux を 1 回撃って stdout を得る。起動失敗・rc 非 0 はいずれも `None`。
fn tmux_stdout(socket: Option<&str>, args: &[&str]) -> Option<String> {
    let mut command = Command::new("tmux");
    if let Some(path) = socket {
        command.arg("-S").arg(path);
    }
    let out = command.args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 入力欄の門の断り。**閉じた 2 値**（憲法 C11: 境界ごとの enum・字面で routing しない）。
///
/// `seat/inject.rs` から**挙動不変で移した**もの（`s2-07l.479.3`）: 注入の口は ADR-0045 §2 (2) で
/// 消えたが、この型は shell の入力欄の門（[`shell_input_empty`]）と口座の delivery
/// （`account login`・ADR-0045 §2 (4) の不変の面）と立て直しが返し続ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputGate {
    /// 入力欄が非空（人間が打ちかけている）。
    Busy,
    /// prompt 行を特定できない。
    UnknownInput,
}

/// tmux を撃てなかった（同じ便で `seat/inject.rs` から移した・読み手は口座の delivery と立て直し）。
pub const REASON_TMUX_FAILED: &str = "tmux-failed";

/// pane の前面 process を shell と読む名（`#{pane_current_command}` の値・閉じた列・字面は現物が正本）。
pub const SHELLS: &[&str] = &["sh", "bash", "zsh", "fish"];

/// target の pane の前面 process が shell か＝session が終わっているか（設計 seat-state.md §6・account-autonomy.md §5
/// の立て直しの入口 (3)）。`list-panes -F '#{pane_current_command}'` を target で引く（typed な metadata・端末描画の
/// 字面ではない＝C3.3 の外）。**window の全 pane が [`SHELLS`] のどれかの周だけ真**で、撃てない・pane が無い・shell で
/// ない pane が 1 つでも在る周は偽（起こし直さない側・fail-closed）。
pub fn pane_is_shell(socket: Option<&str>, target: &str) -> bool {
    let Some(out) = tmux_stdout(socket, &["list-panes", "-t", target, "-F", "#{pane_current_command}"]) else {
        return false;
    };
    let names: Vec<&str> = out.lines().map(str::trim).filter(|name| !name.is_empty()).collect();
    !names.is_empty() && names.iter().all(|name| SHELLS.contains(name))
}

/// tmux を 1 回撃ち、成功したかだけを見る（出力を持たない send 系に使う）。
pub fn tmux_ok(socket: Option<&str>, args: &[&str]) -> bool {
    let mut command = Command::new("tmux");
    if let Some(path) = socket {
        command.arg("-S").arg(path);
    }
    command
        .args(args)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// pane 本文を capture する。撃てなければ `None`（**空文字と区別する**）。
pub fn capture(socket: Option<&str>, target: &str) -> Option<String> {
    tmux_stdout(socket, &["capture-pane", "-p", "-t", target])
}

/// pane id（`%N`・生成 hooks.json の shell 行が `$TMUX_PANE` から渡す）から、tick と同じ形の
/// target（`session:window`）を解く（設計 seat-state.md §3・ADR-0015 §2.2）。撃てない・どちらかの
/// 名が空は `None`（打刻しない側）。
///
/// **両方の名が非空のときだけ** target とする: 無い pane id を渡された `display-message` は版に
/// よって rc 非 0 でなく **`:`（両方空）を rc 0 で返す**（実測 2026-09-11・tmux 3.6b）。空を通すと
/// 潰した dir 名 `_` の席が生まれ、存在しない席へ打刻を積む。
pub fn target_of_pane(socket: Option<&str>, pane: &str) -> Option<String> {
    let out = tmux_stdout(
        socket,
        &["display-message", "-p", "-t", pane, "#{session_name}:#{window_name}"],
    )?;
    let target = out.trim();
    let (session, window) = target.split_once(':')?;
    (!session.is_empty() && !window.is_empty()).then(|| target.to_owned())
}

/// **呼び手自身の** pane の session の名（設計 seat-roles.md §26 の約束 3）: `-t` を**付けない**
/// `display-message -p '#{session_name}'` の 1 問いで、tmux 自身が呼び手の pane を解く。
///
/// **環境変数は 1 つも読まない**（憲法 C2.2・器の `env::` の許し列は `args` / `args_os` / `current_dir` の 3 つ
/// だけで、`TMUX_PANE` を読む形は構造検査が落とす）。tmux の外で撃った周・撃てない周・名が空の周は `None`
/// （呼び側は既定を解かず断る）。
pub fn session_of_caller(socket: Option<&str>) -> Option<String> {
    let out = tmux_stdout(socket, &["display-message", "-p", "#{session_name}"])?;
    let name = out.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// **呼び手自身の** pane の target（`session:window`・設計 seat-roles.md §26 の約束 7）: `-t` を**付けない**
/// 同じ 1 問いで、起動の口を打った窓そのものを名で測る。
///
/// 読み方は [`target_of_pane`] と同じ（**両方の名が非空のときだけ** target・`:` だけを rc 0 で返す版に
/// 空を通さない）で、pane id を引数で受け取らない点だけが違う。
pub fn target_of_caller(socket: Option<&str>) -> Option<String> {
    let out = tmux_stdout(socket, &["display-message", "-p", "#{session_name}:#{window_name}"])?;
    let target = out.trim();
    let (session, window) = target.split_once(':')?;
    (!session.is_empty() && !window.is_empty()).then(|| target.to_owned())
}

/// 最後の prompt 行の右（入力欄）の字面。prompt 行が無ければ `None`。
///
/// `None` は「入力欄が空」ではなく **特定できない**である（呼び側は fail-closed に
/// 倒す＝送らない）。
pub fn input_tail(pane: &str) -> Option<&str> {
    pane.lines()
        .rfind(|line| line.contains(PROMPT))?
        .split_once(PROMPT)
        .map(|(_, right)| right.trim())
}

/// shell の prompt の末尾（**閉じた列**・宣言順・字面は現物が正本・憲法 C2）。立て直しの注入は shell の pane に
/// 撃つので、席の [`PROMPT`] では入力欄を特定できない（設計 account-autonomy.md §5「shell への注入の門」）。
pub const SHELL_PROMPT_TAILS: &[&str] = &["$ ", "# ", "% ", "> "];

/// shell の pane の入力欄の門（pure・`s2-07l.218`）: 可視域の**最後の非空行**が [`SHELL_PROMPT_TAILS`] のどれかで
/// 終わる（右端の空白は trim しない＝prompt の直後に字が無い）周だけ `Ok`。その行が末尾のどれかを途中に含む
/// （prompt の後に打ちかけが在る）周は [`InputGate::Busy`]、含まない（`Password:` 等・空 pane）周は
/// [`InputGate::UnknownInput`]——どちらも 1 key も送らない側（C11.2・緩めない）。
///
/// 席の `❯` の行は読まない: 終了した席の画面に残る古い `❯` 行は shell の入力欄ではない（[`inject::guard_input`] は
/// 席の pane 用のまま）。
pub fn shell_input_empty(pane: &str) -> Result<(), InputGate> {
    let Some(last) = pane.lines().rfind(|line| !line.trim().is_empty()) else {
        return Err(InputGate::UnknownInput);
    };
    if SHELL_PROMPT_TAILS.iter().any(|tail| last.ends_with(tail)) {
        Ok(())
    } else if SHELL_PROMPT_TAILS.iter().any(|tail| last.contains(tail)) {
        Err(InputGate::Busy)
    } else {
        Err(InputGate::UnknownInput)
    }
}

/// statusline を探す域: 最後の prompt 行より下の非空行（prompt 不在なら末尾 6 非空行）。
pub fn search_region(pane: &str) -> Vec<&str> {
    let lines: Vec<&str> = pane.lines().collect();
    match lines.iter().rposition(|line| line.contains(PROMPT)) {
        Some(at) => lines
            .iter()
            .skip(at.saturating_add(1))
            .filter(|line| !line.trim().is_empty())
            .copied()
            .collect(),
        None => tail_nonempty(pane),
    }
}

/// prompt の位置に依らない**直近 [`TAIL_LINES`] 非空行**（prompt 不在の pane の statusline 探索域）。
pub fn tail_nonempty(pane: &str) -> Vec<&str> {
    let mut tail: Vec<&str> = pane
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(TAIL_LINES)
        .collect();
    tail.reverse();
    tail
}

/// prompt が 1 行も無い pane で末尾から見る行数。
const TAIL_LINES: usize = 6;

/// target を file 名に使える字面へ潰す（`[A-Za-z0-9_.-]` 以外は `_`）。
///
/// **潰した結果が `.` か `..` になった周は全部 `_` にする**。この 2 形は path の
/// component として上へ抜けるので、`<state_dir>/seat/<target>/tick.jsonl` が
/// `<state_dir>/tick.jsonl` になり **state dir の外を書く**（多段の `../x` は `.._x` に
/// 潰れるので抜けない）。長さは変えない（何文字を潰したかを残す）。
pub fn sanitize_target(target: &str) -> String {
    let squashed: String = target
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if squashed == "." || squashed == ".." {
        return "_".repeat(squashed.len());
    }
    squashed
}

/// 席の置き場（`<state_dir>/seat/<潰した target>/`）。
///
/// 便 2 の記録 file の**親**から導く＝dir 名の字面を 2 面に持たない。写して持つと片方だけ
/// 変わったときに heartbeat と記録が別の dir へ散り、鮮度が永久に stale になる。
pub fn seat_dir(state_dir: &Path, target: &str) -> PathBuf {
    let path = inject::tick_path(state_dir, target);
    path.parent().map_or_else(|| path.clone(), Path::to_path_buf)
}

/// 置き場の解決の出所（語彙 Provenance・憲法 C10）。**2 値で閉じる**（解決順序 `--state-dir` >
/// git 設定の 2 経路しか無く、第 3 の経路を足すときは variant を足す＝行の `source=` が経路の
/// 全数を名乗る・憲法 C2）。
#[derive(Clone, Copy)]
pub enum Provenance {
    /// `--state-dir` で渡された。
    Flag,
    /// git の設定解決（`git config --get <NAME>.stateDir`）から読んだ。**repo-local に限らない**
    /// （global や git 自身の env 経由の設定も同じ 1 語で名乗る＝器は git の解決を分解しない）。
    GitConfig,
}

impl Provenance {
    /// 行と記録に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::GitConfig => "git-config",
        }
    }
}

/// 解決した置き場（**出所付き**・憲法 C10）。
///
/// 成功行が「書いた」だけでなく「どこへ・何から解いて」を名乗るための型である。
/// rc 0 の成功行が別 dir へ書いていた事故（2026-09-10・repo-local の git 設定が死んだ
/// probe dir を指したまま残っていた）を、行の側で見える形にする（`s2-07l.70`）。
pub struct StateDir {
    /// 解決した path（**絶対**にして持つ: 相対の flag は cwd に依存し「どこへ」を名乗れない）。
    pub path: PathBuf,
    /// 解決の出所。
    pub source: Provenance,
}

impl StateDir {
    /// 成功行と記録の末尾に足す字面（既存 token の後ろ＝名前・順序・書式を変えない）。
    ///
    /// **path は行末**に置く: path は行で唯一潰さない外部の字面で、空白や ` source=` を含みうる。
    /// 出所を先に出せば、読み手は「` state_dir=` 以降の全部が path」と一意に読める。
    pub fn suffix(&self) -> String {
        format!(
            " source={} state_dir={}",
            self.source.as_str(),
            self.path.display()
        )
    }

    /// この置き場の host の受付札の置き場（[`host_slots_dir`]）。
    pub fn slots_dir(&self) -> PathBuf {
        host_slots_dir(&self.path)
    }
}

/// host 単位の受付札の置き場（`<state_dir の親>/<NAME>-host/slots/`・設計 gate-cost.md §3.2）。
///
/// **state dir の親から導く**——同じ host の state dir は 1 つの親（host の state root）に置く
/// 運用なので、project をまたいで 1 つの dir になる。env（`XDG_RUNTIME_DIR` / `HOME` /
/// `TMPDIR`）は読まない（憲法 C2.2）。親を持たない path（`/`）はそれ自身を親と読む。
pub fn host_slots_dir(state_dir: &Path) -> PathBuf {
    state_dir
        .parent()
        .unwrap_or(state_dir)
        .join(format!("{}-host", crate::name::NAME))
        .join("slots")
}

/// 置き場を解く。`--state-dir` が上書きし、無ければ repo の git 設定から読む。
///
/// `current_dir` は syscall であって env ではない（憲法 C2.2・hook 側と同じ扱い）。
/// 絶対化は `std::path::absolute`（symlink も存在も見ない＝書く先そのものの名前）。
pub fn state_dir_of(state_dir: Option<&str>) -> Option<StateDir> {
    let (path, source) = match state_dir {
        Some(found) => (PathBuf::from(found), Provenance::Flag),
        None => {
            let cwd = std::env::current_dir().ok()?;
            let root = crate::hook::vessel::repo_root(&cwd)?;
            (crate::hook::vessel::state_dir(&root)?, Provenance::GitConfig)
        }
    };
    let path = std::path::absolute(path).ok()?;
    Some(StateDir { path, source })
}

/// pane 本文を得る。`capture_file` が在れば tmux を **1 度も呼ばない**。
///
/// 明示された file は tmux の**代わり**であって候補ではない: 読めない周に tmux へ落ちると、
/// 「tmux を呼ばない」ための口が live な server を撃つ経路に化ける（meter と同じ扱い）。
pub fn pane_of(socket: Option<&str>, target: &str, capture_file: Option<&str>) -> Option<String> {
    match capture_file {
        Some(path) => std::fs::read_to_string(path).ok(),
        None => capture(socket, target),
    }
}

/// 退避物の名前の前置き（FR23・[`externalize`] と共有）。
pub const WM_PREFIX: &str = "working-memory.";
/// 退避物の名前の後置き。
pub const WM_SUFFIX: &str = ".md";
/// consume 済みの後置き（**mv が consume の実体**ゆえ、この字で終わらない `.md` が未 consumed）。
pub const WM_CONSUMED: &str = ".consumed.md";
/// frontmatter の区切り。
pub const FRONTMATTER: &str = "---";
/// frontmatter を読む上限（byte）。**全文を読まない**（退避物は数十 KB になる）。
const FRONTMATTER_CAP: u64 = 8192;
/// 席を名乗る frontmatter の key。
pub const SEAT_KEY: &str = "seat:";

/// 自席の未 consumed 退避物の数え（憲法 C11: 「0 件」と「読めない」を混ぜない）。
pub enum WmScan {
    /// 自席の未 consumed が在る（件数）。
    Unconsumed(usize),
    /// **確認した上で** 0 件。
    None,
    /// dir を読めない＝0 件に潰さない。
    Unreadable,
}

/// `<wm-dir>` の未 consumed 退避物のうち、**frontmatter の `seat:` が `target` と一致**
/// するものを数える（裁定 (c)）。
///
/// file 名の sid で弁別しないのは、席の外から回る tick が sid を知らないためである。
/// 名乗りを持たない退避物は自席のものと数えない——他席の退避物を自席の根拠にすると、
/// **別の席の文脈で `/clear` を撃つ**ことになる。
pub fn scan_wm(dir: &Path, target: &str) -> WmScan {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return WmScan::Unreadable;
    };
    let mut found = 0_usize;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_unconsumed_name(&name) {
            continue;
        }
        if seat_of(&entry.path()).is_some_and(|seat| seat == target) {
            found = found.saturating_add(1);
        }
    }
    if found == 0 {
        WmScan::None
    } else {
        WmScan::Unconsumed(found)
    }
}

/// 未 consumed の退避物の名前か（`working-memory.*.md` かつ `.consumed.md` で終わらない・[`consume`] と共有）。
pub(crate) fn is_unconsumed_name(name: &str) -> bool {
    name.len() >= WM_PREFIX.len().saturating_add(WM_SUFFIX.len())
        && name.starts_with(WM_PREFIX)
        && name.ends_with(WM_SUFFIX)
        && !name.ends_with(WM_CONSUMED)
}

/// 退避物の frontmatter が名乗る席。名乗りが無い・読めないなら `None`（[`externalize`] の carry 元の
/// 弁別もこの 1 本を通る＝自席の数え方を 2 面に持たない）。
pub fn seat_of(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut head = Vec::new();
    Read::read_to_end(&mut file.take(FRONTMATTER_CAP), &mut head).ok()?;
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.lines();
    // 先頭が区切りでない file は frontmatter を持たない（本文の `seat:` を拾わない）。
    if lines.next()?.trim() != FRONTMATTER {
        return None;
    }
    lines
        .take_while(|line| line.trim() != FRONTMATTER)
        .find_map(|line| line.trim().strip_prefix(SEAT_KEY))
        .map(|value| value.trim().trim_matches('"').to_owned())
}

/// 埋め込み manifest の rules 行を読めなかった理由（**閉じた列**・憲法 C11.2 / C11.3・`s2-07l.205`）。
///
/// 「manifest そのものが読めない」を「行が無い」に潰さない: 埋め込みは tracked で build 時に焼く
/// ので parse の失敗は器の欠陥であり、呼び手が既定値・no-rule の分岐へ倒れると欠陥が記録から
/// 消える（NFR4「読めない周は黙って落とさない」）。呼び手はどれも「判定しない側」のままで、
/// **variant の名を理由行に添えるだけ**（極性は変えない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleRead {
    /// 埋め込み manifest を parse できない（`Manifest::embedded` の Err）。
    ManifestUnreadable,
    /// その id の行が無い。
    Missing,
    /// 行は在るが `enabled = false`。
    Disabled,
    /// 行は発効しているが値が整数でない。
    NotInt,
    /// 行は発効しているが値が文字列でない（設計 seat-roles.md §19・役割の既定の読み手）。
    NotStr,
    /// 行は発効し値は文字列だが、字面が閉じた表に無い（`Model` / `Effort` の `parse` の失敗）。
    /// 綴り違いを「行が無い」に潰さない（NFR4）。
    NotInTable,
}

impl RuleRead {
    /// 理由行に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManifestUnreadable => "manifest-unreadable",
            Self::Missing => "missing",
            Self::Disabled => "disabled",
            Self::NotInt => "not-int",
            Self::NotStr => "not-str",
            Self::NotInTable => "not-in-table",
        }
    }

    /// 既存の理由 `no-rule`（[`meter::REASON_NO_RULE`] / [`cycle::REASON_NO_RULE`]）に variant を添えた
    /// 字面（`no-rule:<variant>`）。`&'static str` の理由を運ぶ既存の型（`Cycle::Refused` /
    /// `Context::Unmeasured` 等）をそのまま使うために、組んだ字面を literal で持つ（in-file の歯が
    /// `no-rule:` + [`Self::as_str`] と 1 面であることを pin する）。
    pub fn no_rule(self) -> &'static str {
        match self {
            Self::ManifestUnreadable => "no-rule:manifest-unreadable",
            Self::Missing => "no-rule:missing",
            Self::Disabled => "no-rule:disabled",
            Self::NotInt => "no-rule:not-int",
            Self::NotStr => "no-rule:not-str",
            Self::NotInTable => "no-rule:not-in-table",
        }
    }
}

/// [`RuleRead`] の全 variant（宣言順・歯の網羅の母集団）。
pub const RULE_READS: &[RuleRead] = &[
    RuleRead::ManifestUnreadable,
    RuleRead::Missing,
    RuleRead::Disabled,
    RuleRead::NotInt,
    RuleRead::NotStr,
    RuleRead::NotInTable,
];

/// manifest の読み（parse の結果）を typed な理由へ写す（pure・in-file の歯の入口）。
///
/// parse の Err（`Vec<RuleError>`・行ごとの欠陥）は 1 語 [`RuleRead::ManifestUnreadable`] に畳む:
/// 欠陥の列挙は `rules validate` の面が持ち、席の判定行は「読めなかった」だけを名乗る。
pub fn manifest_read(
    read: Result<crate::rules::manifest::Manifest, Vec<crate::rules::RuleError>>,
) -> Result<crate::rules::manifest::Manifest, RuleRead> {
    read.map_err(|_| RuleRead::ManifestUnreadable)
}

/// 埋め込み manifest を読む（読めない周は [`RuleRead::ManifestUnreadable`]）。
///
/// 席の面（tick / cycle / meter）と封じ込め（`pipe::confine`）が埋め込みを読む **1 本の口**。
pub fn embedded_manifest() -> Result<crate::rules::manifest::Manifest, RuleRead> {
    manifest_read(crate::rules::manifest::Manifest::embedded())
}

/// 発効している rules 行の整数値。読めない周は理由付きの Err（＝呼び手は判定しない側へ倒す）。
///
/// 閾値の**値は code に焼かない**（憲法 C5・C1「規則はデータ」）。tick と cycle が同じ
/// 読み方をするので、読みはここ 1 箇所に置く。
pub fn int_rule(id: &str) -> Result<u64, RuleRead> {
    int_rule_of(&embedded_manifest()?, id)
}

/// 口座の逼迫度の閾値（使用率の百分率）を宣言する rules 行の id（account-autonomy.md §3・値は code に
/// 焼かない・C5）。**読み手は `seat launch` の初回の選定（[`cycle::launch`]）だけになった**（ADR-0045 §2 (2)
/// で管理 tick が消え、行そのものは §2 (4) の「便用の口座選定」として残る）。
pub const ID_THRESHOLD: &str = "R-C9-1";

/// **渡された manifest** から発効している rules 行の整数値を読む（pure・in-file の歯の入口）。
/// 不在 / 不発効 / 整数でない、をそれぞれ別の variant で返す。
pub fn int_rule_of(manifest: &crate::rules::manifest::Manifest, id: &str) -> Result<u64, RuleRead> {
    let row = manifest.get(id).ok_or(RuleRead::Missing)?;
    if !row.enabled {
        return Err(RuleRead::Disabled);
    }
    match row.value {
        crate::rules::RuleValue::Int(found) => Ok(found),
        _ => Err(RuleRead::NotInt),
    }
}

#[cfg(test)]
mod tests {
    use super::InputGate;
    use super::{
        host_slots_dir, int_rule_of, manifest_read, shell_input_empty, Provenance, RuleRead, StateDir, RULE_READS,
        SHELL_PROMPT_TAILS,
    };
    use crate::order::is_declaration_order;
    use crate::rules::manifest::Manifest;
    use proptest::prelude::*;
    use proptest::test_runner::Config;
    use std::path::{Path, PathBuf};

    /// 歯の fixture の行 id（tracked manifest の id を写さない＝行が動いても歯は動かない）。
    const FIXTURE_ID: &str = "fixture.int";

    /// `[[rule]]` 1 行の fixture（kind は整数形の 1 つ・値と発効は引数）。
    fn manifest_with(kind: &str, value: &str, enabled: bool) -> Manifest {
        let text = format!(
            "schema = 1\n\n[[rule]]\nid = \"{FIXTURE_ID}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = {enabled}\nruling = \"r\"\nruled_at = \"d\"\n"
        );
        match Manifest::parse(&text) {
            Ok(found) => found,
            Err(errors) => panic!("fixture の manifest を読める: {errors:?}"),
        }
    }

    /// `int_rule_of` は 4 つの読みを別の variant で返す（不在 / 不発効 / 整数でない / 正常）。
    ///
    /// 3 つの Err を **1 つに潰す変異**（`.ok_or(Missing)` で全部を Missing に、`enabled` を見ない、
    /// 形を見ない）はどれかの assert で落ちる。
    #[test]
    fn rule_read_int_rule_of_names_each_failure() {
        let absent = match Manifest::parse("schema = 1\n") {
            Ok(found) => found,
            Err(errors) => panic!("空の manifest を読める: {errors:?}"),
        };
        assert_eq!(int_rule_of(&absent, FIXTURE_ID), Err(RuleRead::Missing), "行が無い");
        assert_eq!(
            int_rule_of(&manifest_with("SeatCycleSettleS", "7", false), FIXTURE_ID),
            Err(RuleRead::Disabled),
            "行は在るが不発効"
        );
        assert_eq!(
            int_rule_of(&manifest_with("DialogueSurface", "\"orchestrator\"", true), FIXTURE_ID),
            Err(RuleRead::NotInt),
            "発効しているが整数でない"
        );
        assert_eq!(int_rule_of(&manifest_with("SeatCycleSettleS", "7", true), FIXTURE_ID), Ok(7), "正常");
        // 不発効かつ整数でない行は**不発効**が先（発効を見てから形を見る）。
        assert_eq!(
            int_rule_of(&manifest_with("DialogueSurface", "\"orchestrator\"", false), FIXTURE_ID),
            Err(RuleRead::Disabled),
            "不発効が形より先"
        );
    }

    /// `as_str` の全 variant を網羅 match で pin する（列の宣言順・字面の重複なし・`no_rule` は
    /// `no-rule:` + `as_str` と 1 面）。
    #[test]
    fn rule_read_as_str_covers_every_variant() {
        assert!(is_declaration_order(RULE_READS, |read| read as usize), "RULE_READS は宣言順");
        assert_eq!(RULE_READS.len(), 6, "母集団（`.433` で +2＝役割の既定の読み手の 2 理由）");
        for read in RULE_READS.iter().copied() {
            let want = match read {
                RuleRead::ManifestUnreadable => "manifest-unreadable",
                RuleRead::Missing => "missing",
                RuleRead::Disabled => "disabled",
                RuleRead::NotInt => "not-int",
                RuleRead::NotStr => "not-str",
                RuleRead::NotInTable => "not-in-table",
            };
            assert_eq!(read.as_str(), want, "{read:?}");
            assert_eq!(read.no_rule(), format!("{}:{}", super::cycle::REASON_NO_RULE, read.as_str()), "{read:?}");
            let same = RULE_READS.iter().filter(|other| other.as_str() == read.as_str()).count();
            assert_eq!(same, 1, "字面 {} が重複する", read.as_str());
        }
    }

    /// parse に失敗する text からの読みは `ManifestUnreadable` 1 語に畳み、読める text は通す。
    #[test]
    fn rule_read_manifest_read_maps_parse_failure_to_unreadable() {
        assert_eq!(
            manifest_read(Manifest::parse("schema = 1\n\n[[rule]]\nid = \"x\"\n")).map(|_| ()),
            Err(RuleRead::ManifestUnreadable),
            "欠けた行は読めない"
        );
        assert_eq!(manifest_read(Manifest::parse("こわれ\n")).map(|_| ()), Err(RuleRead::ManifestUnreadable));
        assert_eq!(manifest_read(Manifest::parse("schema = 1\n")).map(|_| ()), Ok(()), "空の manifest は読める");
    }

    // flip-check: retroactive s2-07l.223
    /// `StateDir::slots_dir` は Default（空 path）でなく、置き場の**親**の下の固定の相対 path
    /// （`<親>/<NAME>-host/slots`）＝ [`host_slots_dir`] と 1 面。
    #[test]
    fn mutant_in_seat_slots_dir_is_under_the_state_root_not_default() {
        let state = StateDir { path: PathBuf::from("/srv/state/project"), source: Provenance::Flag };
        let expected = Path::new("/srv/state").join(format!("{}-host", crate::name::NAME)).join("slots");
        assert_eq!(state.slots_dir(), expected);
        assert_eq!(state.slots_dir(), host_slots_dir(&state.path), "1 面");
        assert_ne!(state.slots_dir(), PathBuf::default(), "空 path ではない");
        assert!(state.slots_dir().strip_prefix("/srv/state").is_ok(), "置き場の親の下");
    }

    /// 末尾 4 種の各々は prompt の直後に字が無い周だけ通り、打ちかけは `Busy`・prompt 末尾で終わらない行は
    /// `UnknownInput`（`s2-07l.218`）。列の字面と宣言順も pin する。
    #[test]
    fn seat_relaunch_shell_input_each_tail_passes_only_when_empty() {
        assert_eq!(SHELL_PROMPT_TAILS, ["$ ", "# ", "% ", "> "]);
        for (prompt, tail) in [("user@host:dir$ ", "$ "), ("root@host:/# ", "# "), ("host% ", "% "), ("PS C:\\> ", "> ")] {
            assert_eq!(shell_input_empty(prompt), Ok(()), "{tail}");
            assert_eq!(shell_input_empty(&format!("old output\n\n{prompt}\n\n")), Ok(()), "{tail}: 下の空行は読まない");
            assert_eq!(shell_input_empty(&format!("{prompt}git st")), Err(InputGate::Busy), "{tail}: 打ちかけ");
        }
    }

    /// 空 pane・空白だけの pane・prompt 末尾で終わらない行は特定できない。
    #[test]
    fn seat_relaunch_shell_input_unknown_without_a_prompt_tail() {
        for pane in ["", "\n\n", "   \n \t\n", "[sudo] password for user:", "Password:", "user@host:dir$"] {
            assert_eq!(shell_input_empty(pane), Err(InputGate::UnknownInput), "{pane:?}");
        }
    }

    /// 右端の空白は trim しない: `$` の直後に空白が無い行は一致せず、空白が 2 つ（打った空白）は打ちかけ。
    #[test]
    fn seat_relaunch_shell_input_does_not_trim_the_right_edge() {
        assert_eq!(shell_input_empty("user@host:dir$\n"), Err(InputGate::UnknownInput));
        assert_eq!(shell_input_empty("user@host:dir$  "), Err(InputGate::Busy));
    }

    /// 古い `❯` 行は読まない（最後の非空行だけが門の入力）。
    #[test]
    fn seat_relaunch_shell_input_ignores_a_stale_seat_prompt() {
        assert_eq!(shell_input_empty("\u{276f} /exit\nuser@host:dir$ "), Ok(()));
        assert_eq!(shell_input_empty("user@host:dir$ \n\u{276f} "), Err(InputGate::UnknownInput));
    }

    /// 反例の永続化を切り、case 数を 256 に pin する（`tests/e2e/prop.rs` と同じ形）。
    fn config() -> Config {
        Config {
            cases: 256,
            failure_persistence: None,
            ..Config::default()
        }
    }

    proptest! {
        #![proptest_config(config())]

        /// 任意の行列で `Ok` なら最後の非空行が末尾のどれかで終わる。
        #[test]
        fn seat_relaunch_shell_input_ok_implies_last_line_ends_with_a_tail(
            lines in prop::collection::vec("[ a-z$#%>:\u{276f}]{0,12}", 0..6)
        ) {
            let pane = lines.join("\n");
            if shell_input_empty(&pane).is_ok() {
                let last = pane.lines().rfind(|line| !line.trim().is_empty()).unwrap_or_default();
                prop_assert!(SHELL_PROMPT_TAILS.iter().any(|tail| last.ends_with(tail)));
            }
        }
    }
}
