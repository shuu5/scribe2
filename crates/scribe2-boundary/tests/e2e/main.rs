//! 統合 test の唯一の target（憲法 R-C13-2「統合 test file 3 以下」は cargo の
//! integration test **target** の数で数える。以後の leg は module で足す）。
//!
//! doctor の導入先の行（consumer-sync.md §4・AC31・接頭辞 `doctor_consumer_`・`s2-07l.303`）の歯はこの file が持つ
//! （登録は core の `register` で積み、tmux を立てない）。`host init` と doctor の `host-template=` の行（host-init.md §3・
//! 行 a・接頭辞 `host_init_`）の歯と、doctor の `init=` の行（同 §6・行 d・接頭辞 `doctor_init_`・tmux は PATH の先頭の偽 tmux）の歯も
//! この file が持つ（git の global 設定は `GIT_CONFIG_GLOBAL` で toy の file に向ける）。

mod fleet;
mod headless;
mod hook;
mod ledger;
mod ledger_form;
mod ledger_memo;
mod notify;
mod pipe;
mod polarity;
mod prop;
mod rules;
mod seat;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use vessel::fleet::Registration;
use vessel::hook::vessel::digest::{self, PluginRecord};
use vessel::name::{NAME, PLUGIN_DIR};
use vessel::seat::ledger::DEFAULT_BD;
use vessel::seat::role::Role;

/// 同一 process 内での dir 名衝突を避ける連番。
static SEQ: AtomicU32 = AtomicU32::new(0);

/// 起動の実物を据えたか（process に 1 回だけ据える）。
static SPAWNER: OnceLock<bool> = OnceLock::new();

/// core の lib の起動の site に同じ process で届く歯が、site に届く前に撃つ共通の据え付け（bin の main と同じ
/// 実物を process に 1 回だけ据える・設計 core-boundary.md §9 採る形 4）。libtest には全歯の前に走る入口が無い
/// ので、census（recent の git の読みを直に呼ぶ歯・fleet の host の読みを呼ぶ歯・ratelimit の host を読む
/// fixture の helper）の各々が先頭で呼ぶ。
pub fn install_spawner() {
    SPAWNER.get_or_init(scribe2_boundary::spawner::install);
}

/// repo の外に一意な tmp dir を作る。
///
/// `tempfile` は直接依存の追加（憲法 A3）に当たるので足さない。xtask の
/// `make_tmp_dir` と同形の std だけの helper である。返すのは [`TmpDir`]（drop で dir を再帰削除する包み・
/// 設計 docs/design/gate-cost.md §17・行 h・`s2-07l.343`）で、panic した歯も dir を残さない。
///
/// 先頭で [`install_spawner`] を呼ぶ（この helper を使う歯が core の lib の起動の site に同じ process で届く周の
/// 据え付け・census の外で `pipe` の git の読みに届く polarity の worktree の判定の歯がこの helper を先に呼ぶ）。
pub fn make_tmp_dir() -> Option<TmpDir> {
    install_spawner();
    let base = std::env::temp_dir();
    for _ in 0..8 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("e2e-{}-{nanos}-{seq}", std::process::id()));
        if std::fs::create_dir(&dir).is_ok() {
            return Some(TmpDir { path: Some(dir) });
        }
    }
    None
}

/// 歯の fixture の一時 dir の包み（`Drop` で再帰削除・panic の unwind の途中でも消える）。
///
/// path として読める（`Deref` で `Path` を貸す）ので、path を繋ぐだけの呼び手は素の `PathBuf` と同じに書ける。
/// 落ちた歯の dir を調べたい周は [`TmpDir::keep`] で path を取り出して guard を降ろす（env は読まない・C2.2）。
#[derive(Debug)]
pub struct TmpDir {
    /// 消す dir（`None` = guard を降ろした後・`keep` の中だけが `None` にする）。
    path: Option<PathBuf>,
}

impl TmpDir {
    /// 包みの path（`Deref` の実体・`PathBuf::as_path` と同じ名＝呼び手の字面を変えない）。
    pub fn as_path(&self) -> &Path {
        self.path.as_deref().unwrap_or(Path::new(""))
    }

    /// symlink を解いた path の包みに替える（同じ dir を指す＝消す対象は変わらない・解けなければ `None` で dir は消える）。
    pub fn canonical(mut self) -> Option<Self> {
        let real = self.as_path().canonicalize().ok()?;
        self.path = Some(real);
        Some(self)
    }

    /// path を取り出して guard を降ろす（降ろした周は drop しても dir が残る）。
    pub fn keep(mut self) -> PathBuf {
        self.path.take().unwrap_or_default()
    }

    /// path を取り出し、guard は**いま走っている歯の thread** に預ける（thread の終端＝歯の終わりで drop・panic でも消える）。
    ///
    /// 素の `PathBuf` を返す局所 helper（write-set の外の呼び手が `tmp().join(..)` の形で一時値を捨てる）が使う口である。
    /// 呼び手の文の終わりで包みが落ちると dir が消えてしまうので、寿命を歯 1 本の thread へ延ばす。
    pub fn held(self) -> PathBuf {
        let path = self.as_path().to_path_buf();
        HELD.with(|held| held.borrow_mut().push(self));
        path
    }
}

thread_local! {
    /// [`TmpDir::held`] が預かった包み（thread の終端で drop される＝libtest は歯 1 本を 1 thread で走らせる）。
    static HELD: std::cell::RefCell<Vec<TmpDir>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            fs::remove_dir_all(path).ok();
        }
    }
}

impl std::ops::Deref for TmpDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        self.as_path()
    }
}

impl AsRef<Path> for TmpDir {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl AsRef<std::ffi::OsStr> for TmpDir {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.as_path().as_os_str()
    }
}

// ─────────── 一時 dir の包みの歯（設計 docs/design/gate-cost.md §17・行 h・`s2-07l.343`） ───────────

/// (a) 包みを drop した後に dir が無い（中に file と sub dir を置いた周も再帰で消える）。
#[test]
fn e2e_fixture_tmp_dir_is_removed_on_drop() {
    let dir = make_tmp_dir().unwrap_or_else(|| panic!("tmp dir を作れる"));
    let path = dir.to_path_buf();
    fs::create_dir_all(dir.join("sub")).unwrap_or_else(|e| panic!("sub dir を作れる: {e}"));
    fs::write(dir.join("sub").join("file"), "x\n").unwrap_or_else(|e| panic!("file を置ける: {e}"));
    assert!(path.join("sub").join("file").is_file(), "前提: drop の前は在る");
    drop(dir);
    assert!(!path.exists(), "drop の後に dir が無い: {}", path.display());
}

/// (b) panic した歯でも dir が消える（unwind を捕まえる口の中で作って落とし、外で不在を測る）。預けた包みも
/// thread の終端（panic で落ちた thread を含む）で消える。
#[test]
fn e2e_fixture_tmp_dir_is_removed_when_the_tooth_panics() {
    let made = std::sync::Mutex::new(PathBuf::new());
    let caught = std::panic::catch_unwind(|| {
        let dir = make_tmp_dir().unwrap_or_else(|| panic!("tmp dir を作れる"));
        fs::write(dir.join("file"), "x\n").ok();
        if let Ok(mut slot) = made.lock() {
            *slot = dir.to_path_buf();
        }
        panic!("歯が落ちる");
    });
    assert!(caught.is_err(), "前提: 中で panic した");
    let path = made.lock().map(|slot| slot.clone()).unwrap_or_default();
    assert!(!path.as_os_str().is_empty(), "前提: dir を作った");
    assert!(!path.exists(), "panic の後に dir が無い: {}", path.display());
    let held = std::thread::spawn(|| {
        let dir = make_tmp_dir().unwrap_or_else(|| panic!("tmp dir を作れる")).held();
        std::panic::panic_any(dir);
    })
    .join();
    let path = held.err().and_then(|payload| payload.downcast::<PathBuf>().ok()).map(|path| *path);
    let path = path.unwrap_or_else(|| panic!("前提: 預けた thread が path を運んで panic した"));
    assert!(!path.exists(), "預けた包みも panic した thread の終端で消える: {}", path.display());
}

/// (c) guard を降ろした周は drop の後も dir が在る（降ろす口が無ければ空虚になる pin）。
#[test]
fn e2e_fixture_tmp_dir_survives_when_kept() {
    let dir = make_tmp_dir().unwrap_or_else(|| panic!("tmp dir を作れる"));
    let path = dir.keep();
    assert!(path.is_dir(), "降ろした周は dir が在る: {}", path.display());
    fs::remove_dir_all(&path).ok();
    let canonical = make_tmp_dir().and_then(TmpDir::canonical).unwrap_or_else(|| panic!("正規化できる"));
    let path = canonical.to_path_buf();
    drop(canonical);
    assert!(!path.exists(), "正規化の後も包みが生きて消える: {}", path.display());
}

/// (d) 作り手が 2 回続けて別の path を返す（既存の一意性が壊れていない）。
#[test]
fn e2e_fixture_tmp_dir_paths_are_unique() {
    let first = make_tmp_dir().unwrap_or_else(|| panic!("1 つ目を作れる"));
    let second = make_tmp_dir().unwrap_or_else(|| panic!("2 つ目を作れる"));
    assert_ne!(first.as_path(), second.as_path(), "2 回続けて別の path");
    assert!(first.is_dir() && second.is_dir(), "どちらも在る");
}

// ─────────── 固定日付を持つ fixture の母集団（設計 docs/design/pipeline.md §50・行 as・`s2-07l.469`） ───────────

/// reset / 期限の欄の key（`concat!` で 2 片に割る＝この file の本文が自分の母集団に数えられない）。
const CLOCK_KEYS: [&str; 5] = [
    concat!("resets", "_at"),
    concat!("reset", "_at"),
    concat!("RESETS", "_AT"),
    concat!("_RE", "SET"),
    concat!("expires", "At"),
];

/// `line` の中で引用符の直後に座る日付の形（`YYYY-MM-DD`）の字面のうち最初の 1 つの年（無ければ `None`）。
fn quoted_date_year(line: &str) -> Option<u32> {
    line.as_bytes().windows(11).find_map(|window| match *window {
        [b'"', y0, y1, y2, y3, b'-', m0, m1, b'-', d0, d1]
            if [y0, y1, y2, y3, m0, m1, d0, d1].iter().all(u8::is_ascii_digit) =>
        {
            Some([y0, y1, y2, y3].iter().fold(0, |year, digit| year * 10 + u32::from(digit - b'0')))
        }
        _ => None,
    })
}

/// e2e の tracked な `.rs` の本数と、reset / 期限の key を持つ行のうち日付の形の字面を持つ行を年で 2 つに割った
/// 本数を 3 つ組で pin する。年 2099 以上は番兵（時限にならない）・未満は壁時計と比べれば時限になる字面で、
/// 1 本で 2 本を兼ねないよう別々の欄で持つ。message は母集団の全数と当たった行（file・行番号・年）を出す。
#[test]
fn e2e_fixture_clock_dated_reset_lines_are_pinned() {
// flip-check: retroactive s2-07l.469
    const SENTINEL_YEAR: u32 = 2099;
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let listed = Command::new("git")
        .arg("-C")
        .arg(&crate_dir)
        .args(["ls-files", "--", "tests/e2e"])
        .output()
        .unwrap_or_else(|e| panic!("git ls-files を撃てる: {e}"));
    assert!(listed.status.success(), "git ls-files は rc 0: {}", String::from_utf8_lossy(&listed.stderr));
    let tracked: Vec<String> =
        String::from_utf8_lossy(&listed.stdout).lines().filter(|path| path.ends_with(".rs")).map(str::to_owned).collect();
    let mut hits: Vec<(String, usize, u32)> = Vec::new();
    for path in &tracked {
        let text = fs::read_to_string(crate_dir.join(path)).unwrap_or_else(|e| panic!("{path} を読める: {e}"));
        for (at, line) in text.lines().enumerate() {
            if !CLOCK_KEYS.iter().any(|key| line.contains(key)) {
                continue;
            }
            if let Some(year) = quoted_date_year(line) {
                hits.push((path.clone(), at + 1, year));
            }
        }
    }
    let sentinel = hits.iter().filter(|(_, _, year)| *year >= SENTINEL_YEAR).count();
    let dated = hits.len() - sentinel;
    let mut files: Vec<&str> = hits.iter().map(|(path, _, _)| path.as_str()).collect();
    files.dedup();
    assert_eq!(
        (tracked.len(), sentinel, dated),
        (29, 7, 5),
        "母集団: e2e の tracked な .rs {} 本・当たった行 {} 行（年 {SENTINEL_YEAR} 以上 {sentinel}・未満 {dated}）・\
         file {files:?}・行 {hits:?}",
        tracked.len(),
        hits.len()
    );
}

// ─────────── e2e の歯の道具箱（設計 docs/design/gate-cost.md §30・行 v・`s2-07l.504`） ───────────
//
// 歯が toy repo で実 binary を撃つときの PATH の組み立ては**この 1 関数**（[`toolbox_path`]）に寄る。
// 偽 `systemd-run` と偽 `systemctl` を置いた dir を先頭に積むので、歯の起こす toy の process は
// 実 systemd の scope を 1 本も作らない——scope は slice 直下の平面にしか作れず、実物を撃つと
// gate の箱（§4）から構造的に外れたまま user の systemd を詰まらせる（`s2-07l.504` の実測）。
// 偽にすると toy の process は歯の process の子のまま走る＝gate の箱の中に留まる。

// flip-check: retroactive s2-07l.504
// flip-check: retroactive s2-07l.530
// flip-check: retroactive s2-07l.484

/// 道具箱の偽 binary を置く dir の leaf 名（呼び手の fixture の dir の直下）。
pub const TOOLBOX_BIN: &str = "toolbox-bin";

/// 道具箱の偽 `systemd-run` が argv を写す記録 dir の leaf 名（**1 起動 1 file**）。
///
/// `tests/e2e/pipe/gate.rs` の `SCOPE_RECORDS`（明示の口の記録）とは**別の名**である——同じ置き場に
/// 重ねると、既存の歯の母集団（`scope_record(` の「ちょうど 1 件」）に道具箱の起動まで混ざる。
pub const TOOLBOX_RECORDS: &str = "toolbox-scope-args";

/// 道具箱の記録 dir（[`toolbox_path`] が作る・読み手は dir を走査する）。
pub fn toolbox_records(dir: &Path) -> PathBuf {
    dir.join(TOOLBOX_RECORDS)
}

/// 実行権つきの偽 binary を `shim` の名へ置く（**道具箱の 2 本が両方通る 1 つの手**・設計 §36 形 1〜3・`s2-07l.530`）。
///
/// 目的の名と**同じ dir** の一時の名（目的の名から導く）へ本文を書き、実行権を付けてから、std の fs の rename で
/// 目的の名へ入れ替える。目的の名をその場で切り詰める書き方（`fs::write(&shim, ..)`）にしないのは、走っている
/// process が exec している本体（inode）を書き換えて `ExecutableFileBusy` で落ちるからである（同じ置き場で
/// binary を 2 度撃つ歯・main の CI で 2 回赤・§36 出所）。同じ dir に書くのは file system を跨ぐ rename が
/// 落ちるから、実行権を入れ替えの**前**に付けるのは付く前の本体が目的の名で見える窓を作らないためである。
/// 一時の名は rename で消える＝bin dir の entry は置いた偽 binary の名だけになる。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn install_shim(shim: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let name = shim.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let staged = shim.with_file_name(format!(".{name}.staging"));
    fs::write(&staged, script).expect("stub を書ける");
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755)).expect("stub に実行権を付ける");
    fs::rename(&staged, shim).expect("stub を目的の名へ入れ替える");
}

/// 偽 `systemd-run` を `bin_dir` に 1 本書く（**偽の本体の唯一の生成元**・設計 §30 約束 3）。
///
/// argv を `<unit>.args` の**1 起動 1 file**で `records` へ写してから `--` の後ろを exec する
/// ＝包みの中身は実際に撃たれる。1 file へ追記する形にしないのは、probe の記録や別の行の記録まで
/// 同じ母集団に入り、`contains` の assert が**撃っていない起動の引数**で充足するからである。
///
/// **同じ名の 2 本目は実 systemd と同じ字面で断る**（`s2-07l.234`）——記録を上書きする形だと、
/// 同じ process が同名を 2 度撃つ周が歯に見えない。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
pub fn write_systemd_run_stub(bin_dir: &Path, records: &Path) {
    fs::create_dir_all(bin_dir).expect("stub の dir を作れる");
    fs::create_dir_all(records).expect("記録の dir を作れる");
    let shim = bin_dir.join("systemd-run");
    let script = format!(
        "#!/bin/sh\n\
         __unit=no-unit\n\
         for __a in \"$@\"; do case \"$__a\" in --unit=*) __unit=${{__a#--unit=}};; esac; done\n\
         if [ -e '{0}'/\"$__unit\".args ]; then\n\
         printf 'Failed to start transient scope unit: Unit %s.scope was already loaded or has a fragment file.\\n' \"$__unit\" >&2\n\
         exit 1\n\
         fi\n\
         printf '%s\\n' \"$@\" > '{0}'/\"$__unit\".args\n\
         while [ $# -gt 0 ] && [ \"$1\" != \"--\" ]; do shift; done\n\
         shift\n\
         exec \"$@\"\n",
        records.display()
    );
    install_shim(&shim, &script);
}

/// 道具箱の偽 `systemctl` を `bin_dir` に 1 本書く（設計 §30 約束 4）。
///
/// `kill` は「もう無い」の字面（→ `Released::Gone`＝record に `scope=` を書かない）・`show` は空
/// （→ peak は読まない）を返す。偽が作らなかった unit に実 host が返す答えと同じなので、record の
/// field は増えも減りもしない。
fn write_systemctl_stub(bin_dir: &Path) {
    let shim = bin_dir.join("systemctl");
    let script = "#!/bin/sh\n\
                  case \"$2\" in\n\
                  show) exit 0;;\n\
                  kill) printf 'Failed to kill unit %s: Unit %s not loaded.\\n' \"$4\" \"$4\" >&2; exit 1;;\n\
                  esac\n\
                  exit 1\n";
    install_shim(&shim, script);
}

/// 道具箱の台帳 client の見張りが argv を写す記録 dir の leaf 名（**1 起動 1 file**・設計 §37・`s2-07l.484`）。
///
/// [`TOOLBOX_RECORDS`] とは**別の名**である——同じ置き場に重ねると、systemd-run の記録の母集団
/// （`toolbox_record(` の「ちょうど 1 件」）に台帳の起動が混ざる。
pub const TOOLBOX_LEDGER_RECORDS: &str = "toolbox-ledger-args";

/// 見張りの記録 dir（[`toolbox_path`] が作る・読み手は dir を走査する）。
pub fn toolbox_ledger_records(dir: &Path) -> PathBuf {
    dir.join(TOOLBOX_LEDGER_RECORDS)
}

/// 道具箱に台帳 client の既定名（[`DEFAULT_BD`]）の見張りを 1 本置く（設計 §37 形 1）。
///
/// 呼ばれた argv を `records` の下へ**1 起動 1 file**（mktemp の一意な名）で写してから、台帳を解けない host と
/// 同じ形で断る（標準出力は空・rc は非 0）。rc 0 の空の台帳として答えさせないのは、列の 1 周が「0 件を読めた」へ
/// 倒れて unmeasured reason=ledger の枝が測られなくなるからである（§37 却下）。`--bd` に絶対 path を渡す起動は
/// PATH を通らないので、ここへは届かない。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn write_ledger_tripwire(bin_dir: &Path, records: &Path) {
    fs::create_dir_all(records).expect("見張りの記録の dir を作れる");
    let script = format!(
        "#!/bin/sh\n\
         __f=$(mktemp '{0}'/call.XXXXXXXX) || exit 127\n\
         printf '%s\\n' \"$@\" > \"$__f\"\n\
         exit 127\n",
        records.display()
    );
    install_shim(&bin_dir.join(DEFAULT_BD), &script);
}

/// 見張りの記録 dir の entry 名（昇順・dir が無ければ空＝器は台帳 client を 1 度も起こしていない）。
pub fn toolbox_ledger_record_names(dir: &Path) -> Vec<String> {
    sorted_entry_names(&toolbox_ledger_records(dir))
}

/// 道具箱の記録 dir の entry 名（昇順・dir が無ければ空＝1 件も作っていない）。
pub fn toolbox_record_names(dir: &Path) -> Vec<String> {
    sorted_entry_names(&toolbox_records(dir))
}

/// `dir` の entry 名（昇順・dir が無ければ空）。
fn sorted_entry_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// `needle` を名に含む道具箱の記録の**ちょうど 1 件**の本文（1 行 1 引数）。
///
/// 0 件も 2 件以上も落とすのは、母集団を確かめずに `contains` すると**別の起動の引数**で assert が
/// 充足するからである（fixture 衝突・`tests/e2e/pipe/gate.rs` の `scope_record(` と同じ形）。
pub fn toolbox_record(dir: &Path, needle: &str) -> String {
    let names = toolbox_record_names(dir);
    let hits: Vec<&String> = names.iter().filter(|name| name.contains(needle)).collect();
    assert_eq!(hits.len(), 1, "{needle} の記録はちょうど 1 件（母集団 {names:?}）");
    hits.first()
        .and_then(|name| fs::read_to_string(toolbox_records(dir).join(name)).ok())
        .unwrap_or_default()
}

/// 歯が toy repo で実 binary を撃つときの PATH（設計 §30 約束 1・**3 つの口が全部ここを通る**）。
///
/// 道具箱（偽 `systemd-run` と偽 `systemctl` と台帳 client の見張り）を `dir` の直下に置き、その dir を
/// **先頭に積んだ** PATH の値を返す。host の PATH は後ろに残る（git / sh / cargo の解決は不変）。
pub fn toolbox_path(dir: &Path) -> String {
    let bin_dir = dir.join(TOOLBOX_BIN);
    write_systemd_run_stub(&bin_dir, &toolbox_records(dir));
    write_systemctl_stub(&bin_dir);
    write_ledger_tripwire(&bin_dir, &toolbox_ledger_records(dir));
    format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default())
}

// ─────────────────── doctor の導入先の行（consumer-sync.md §4・AC31・`s2-07l.303`） ───────────────────

/// 導入先の歯の置き場（tmp の root・state dir・vessel repo とその HEAD・plugin root とその hooks.json の digest）。
struct ConsumerPlace {
    dir: TmpDir,
    state: PathBuf,
    vessel: PathBuf,
    head: String,
    root: PathBuf,
    digest: String,
}

/// git を 1 回撃ち、rc 0 なら stdout を返す。
fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// commit を 1 つ持つ git repo を `dir` に作り、HEAD の sha（全桁）を返す。
fn git_repo_at(dir: &Path) -> Option<String> {
    fs::create_dir_all(dir).ok()?;
    git_out(dir, &["init", "-q"])?;
    git_out(dir, &["config", "user.name", "e2e"])?;
    git_out(dir, &["config", "user.email", "e2e@example.invalid"])?;
    fs::write(dir.join("seed"), "seed\n").ok()?;
    git_out(dir, &["add", "-A"])?;
    git_out(dir, &["commit", "-q", "-m", "seed"])?;
    git_out(dir, &["rev-parse", "HEAD"])
}

/// `hooks/hooks.json` を `body` で持つ dir を `dir` に作り、その digest を返す。
fn hooks_at(dir: &Path, body: &str) -> Option<String> {
    fs::create_dir_all(dir.join("hooks")).ok()?;
    fs::write(digest::hooks_path(dir), body).ok()?;
    digest::hooks_digest(dir)
}

/// 置き場を 1 つ作る（vessel repo は `[[vessel]]` を書く周だけ読まれる・plugin root は記録の `root=` に使う）。
fn consumer_place() -> Option<ConsumerPlace> {
    let dir = make_tmp_dir()?.canonical()?;
    let state = dir.join("state");
    fs::create_dir_all(&state).ok()?;
    let vessel = dir.join("vessel");
    let head = git_repo_at(&vessel)?;
    let root = dir.join("plugin-root");
    let digest = hooks_at(&root, "{\"hooks\":{}}\n")?;
    Some(ConsumerPlace { dir, state, vessel, head, root, digest })
}

/// host の面を書く（`[[account]]` を `labels` の順に・`vessel` が在れば `[[vessel]] repo` を 1 行）。
fn write_host(place: &ConsumerPlace, labels: &[&str], vessel: Option<&str>) {
    let mut body = "schema = 1\n".to_owned();
    for label in labels {
        body.push_str(&format!("\n[[account]]\nlabel = \"{label}\"\n"));
    }
    if let Some(repo) = vessel {
        body.push_str(&format!("\n[[vessel]]\nrepo = \"{repo}\"\n"));
    }
    fs::write(place.state.join(vessel::rules::HOST_MANIFEST), body).ok();
}

/// 帳簿の導入先 1 つ（`projectPath` / `scope` / `installPath` / `gitCommitSha`・`None` は key を書かない）。
struct LedgerRow<'a> {
    project: &'a str,
    scope: Option<&'a str>,
    install: Option<&'a str>,
    sha: Option<&'a str>,
}

/// 口座 `label` の帳簿を書く（`plugins["<NAME>@<NAME>"]` に `rows`・他の key は Claude Code の実物の形を写す）。
fn write_ledger(place: &ConsumerPlace, label: &str, rows: &[LedgerRow]) -> PathBuf {
    let items: Vec<String> = rows
        .iter()
        .map(|row| {
            let mut pairs = vec![format!("\"projectPath\":\"{}\"", row.project)];
            for (key, value) in [("scope", row.scope), ("installPath", row.install), ("gitCommitSha", row.sha)] {
                if let Some(found) = value {
                    pairs.push(format!("\"{key}\":\"{found}\""));
                }
            }
            pairs.push("\"version\":\"0.1.0\",\"installedAt\":\"2026-09-15T00:00:00Z\"".to_owned());
            format!("{{{}}}", pairs.join(","))
        })
        .collect();
    let body = format!("{{\"version\":2,\"plugins\":{{\"{NAME}@{NAME}\":[{}]}}}}", items.join(","));
    write_ledger_text(place, label, &body)
}

/// 口座 `label` の帳簿を本文そのままで書く（壊れた帳簿の周）。
fn write_ledger_text(place: &ConsumerPlace, label: &str, body: &str) -> PathBuf {
    let path = vessel::account::consumers::ledger_path(&place.state, label);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&path, body).ok();
    path
}

/// 登録 row を 1 件積む（planner・`anchor` = 導入先の path・tmux は立てない）。
fn register_anchor(place: &ConsumerPlace, anchor: &str, target: &str) {
    let row = Registration {
        role: Role::Orchestrator,
        anchor: anchor.to_owned(),
        target: target.to_owned(),
        sid: None,
        account: "acct-1".to_owned(),
        launch: String::new(),
        model: None,
    };
    assert!(vessel::seat::role::register(&place.state, row).is_ok(), "登録 row を積める");
}

/// 席 `target` の読み込み元の記録を置く（`hooks` は digest か `None` = unreadable・`binary` は build 元 commit の字面）。
fn write_record(place: &ConsumerPlace, target: &str, root: &Path, hooks: Option<&str>, binary: &str) {
    let seat = vessel::seat::seat_dir(&place.state, target);
    fs::create_dir_all(&seat).ok();
    let record = PluginRecord::Recorded {
        root: root.display().to_string(),
        hooks: hooks.map(str::to_owned),
        binary: binary.to_owned(),
        sid: "sid-fix".to_owned(),
        ts: 1_800_000_000,
    };
    fs::write(digest::record_path(&seat), format!("{}\n", record.to_line().unwrap_or_default())).ok();
}

/// `doctor --state-dir` を撃ち（socket は server の無い path）、rc 0 と「導入先の行は口座の行の後ろ」を確かめて
/// `consumer=` の行だけを返す。
fn consumer_lines(place: &ConsumerPlace) -> Vec<String> {
    consumer_lines_on(place, None)
}

/// [`consumer_lines`] を PATH を差し替えて撃つ形（`path` が `None` なら継いだ PATH のまま）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn consumer_lines_on(place: &ConsumerPlace, path: Option<&str>) -> Vec<String> {
    let socket = place.dir.join("no-server-sock").display().to_string();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_scribe2"));
    cmd.args(["doctor", "--state-dir", &place.state.display().to_string(), "--tmux-socket", &socket]);
    if let Some(found) = path {
        cmd.env("PATH", found);
    }
    let out = cmd.output().expect("binary を起動できる");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(0), "doctor は判定しない（rc 0）: {stderr}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let all: Vec<&str> = stdout.lines().collect();
    let accounts = all.iter().rposition(|line| line.starts_with("account=")).unwrap_or(0);
    let first = all.iter().position(|line| line.starts_with("consumer="));
    assert!(first.is_none_or(|at| at > accounts), "導入先の行は口座の行の後ろ: {stdout}");
    all.iter().filter(|line| line.starts_with("consumer=")).map(|line| (*line).to_owned()).collect()
}

/// `head=` に載る HEAD の先頭 12 桁。
fn head12(place: &ConsumerPlace) -> String {
    place.head.chars().take(12).collect()
}

/// (d・AC31) 口座 2 つ（帳簿に導入先 2 つと 3 つ・1 つは worktree の path）と `[[vessel]] repo` を置き、登録 row の席の
/// 記録を 5 形で置く → 導入先ごとに 1 行・path の辞書順・`drift=` が `none` / `binary` / `plugin` / `ledger` / `dual` を
/// それぞれ名指す（1 行の全欄を字面で pin）。base は行が無い（RED）。
#[test]
fn doctor_consumer_lines_name_each_drift_word() {
    let place = consumer_place().unwrap_or_else(|| panic!("置き場を作れる"));
    let (head, sha12, digest) = (place.head.as_str(), head12(&place), place.digest.as_str());
    let vessel = place.vessel.display().to_string();
    write_host(&place, &["acc-a", "acc-b"], Some(&vessel));
    let cache = place.dir.join("cache");
    let cache_digest = hooks_at(&cache, "{\"hooks\":{\"Stop\":[]}}\n").unwrap_or_default();
    let (cache_s, missing) = (cache.display().to_string(), place.dir.join("no-cache").display().to_string());
    let stale = "0".repeat(40);
    write_ledger(
        &place,
        "acc-a",
        &[
            LedgerRow { project: "/c/none", scope: Some("project"), install: Some(&cache_s), sha: Some(head) },
            LedgerRow { project: "/c/binary", scope: Some("local"), install: Some(&cache_s), sha: Some(head) },
        ],
    );
    write_ledger(
        &place,
        "acc-b",
        &[
            LedgerRow { project: "/c/plugin", scope: Some("user"), install: None, sha: Some(head) },
            LedgerRow { project: "/c/ledger/.worktrees/w", scope: Some("project"), install: Some(&missing), sha: Some(&stale) },
            LedgerRow { project: "/c/dual", scope: Some("project"), install: Some(&cache_s), sha: Some(head) },
        ],
    );
    let build = env!("SCRIBE2_BUILD_COMMIT");
    let other = "f".repeat(16);
    let checkout_payload = place.vessel.join(PLUGIN_DIR);
    for (anchor, target, root, hooks, binary) in [
        ("/c/none", "n:n", &place.root, Some(digest), build),
        ("/c/binary", "b:b", &place.root, Some(digest), "000000000000"),
        ("/c/plugin", "p:p", &place.root, Some(other.as_str()), build),
        ("/c/ledger/.worktrees/w", "l:l", &place.root, Some(digest), build),
        ("/c/dual", "d:d", &checkout_payload, None, build),
    ] {
        register_anchor(&place, anchor, target);
        write_record(&place, target, root, hooks, binary);
    }
    let (root, payload) = (place.root.display().to_string(), checkout_payload.display().to_string());
    let lines = consumer_lines(&place);
    let want = [
        format!("consumer=/c/binary source=launch+install scope=local binary=000000000000 plugin={root}:{digest} ledger={head} cache={cache_digest} head={sha12} behind=unmeasured drift=binary"),
        format!("consumer=/c/dual source=launch+install scope=project binary={build} plugin={payload}:unreadable ledger={head} cache={cache_digest} head={sha12} behind=unmeasured drift=dual"),
        format!("consumer=/c/ledger/.worktrees/w source=launch+install scope=project binary={build} plugin={root}:{digest} ledger={stale} cache=absent head={sha12} behind=unmeasured drift=ledger"),
        format!("consumer=/c/none source=launch+install scope=project binary={build} plugin={root}:{digest} ledger={head} cache={cache_digest} head={sha12} behind=unmeasured drift=none"),
        format!("consumer=/c/plugin source=launch+install scope=user binary={build} plugin={root}:{other} ledger={head} cache=absent head={sha12} behind=unmeasured drift=plugin"),
    ];
    assert_eq!(lines, want, "導入先ごとに 1 行・path の辞書順・語は 1 つずつ");
    assert_ne!(cache_digest, digest, "cache は installPath の hooks.json（記録の root とは別の file）");
    fs::remove_dir_all(&place.dir).ok();
}

/// (e) 記録の無い導入先は `unrecorded`（`none` に潰さない）: 登録 row だけ（`source=launch`・`ledger=-`）・帳簿だけ
/// （`source=install`・記録の置き場が無い）・記録の位置に dir（読めない＝不在に潰さない）。帳簿の食い違いは記録が
/// 無くても測り、宣言順に `+` で繋ぐ（`ledger+unrecorded`）。
#[test]
fn doctor_consumer_unrecorded_is_not_none() {
    let place = consumer_place().unwrap_or_else(|| panic!("置き場を作れる"));
    let (head, sha12) = (place.head.as_str(), head12(&place));
    let vessel = place.vessel.display().to_string();
    write_host(&place, &["acc-a"], Some(&vessel));
    let stale = "1".repeat(40);
    write_ledger(
        &place,
        "acc-a",
        &[
            LedgerRow { project: "/u/install", scope: Some("project"), install: None, sha: Some(head) },
            LedgerRow { project: "/u/stale", scope: None, install: None, sha: Some(&stale) },
        ],
    );
    register_anchor(&place, "/u/launch", "u:launch");
    register_anchor(&place, "/u/broken", "u:broken");
    let seat = vessel::seat::seat_dir(&place.state, "u:broken");
    fs::create_dir_all(digest::record_path(&seat)).expect("記録の位置に dir を置ける");
    let lines = consumer_lines(&place);
    let tail = |source: &str, ledger: &str, drift: &str| {
        format!("source={source} scope=- binary=unrecorded plugin=unrecorded ledger={ledger} cache=absent head={sha12} behind=unmeasured drift={drift}")
    };
    let want = [
        format!("consumer=/u/broken {}", tail("launch", "-", "unrecorded")),
        format!("consumer=/u/install source=install scope=project binary=unrecorded plugin=unrecorded ledger={head} cache=absent head={sha12} behind=unmeasured drift=unrecorded"),
        format!("consumer=/u/launch {}", tail("launch", "-", "unrecorded")),
        format!("consumer=/u/stale {}", tail("install", &stale, "ledger+unrecorded")),
    ];
    assert_eq!(lines, want, "記録の無い導入先は unrecorded");
    assert!(!lines.iter().any(|line| line.ends_with("drift=none")), "none に潰さない: {lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (e・形 5・consumer-sync.md §17) 導入先の読み込み元は vessel repo の checkout の生成 dir（`<repo>/<PLUGIN_DIR>`）: そこに在る
/// hooks.json の digest が `plugin=<生成 dir>:<digest>` に出て、帳簿にも同じ path が在る行は `dual` と名指す。root 直下（旧 root）を
/// 記録した席は生成 dir ではないので `dual` にならず、旧 root に hooks.json が無いので `plugin` の食い違い。記録の無い導入先は
/// 従来どおり `unrecorded`（**否定の枝**）。
#[test]
fn plugin_payload_doctor_consumer_reads_the_generated_dir_under_the_checkout() {
    let place = consumer_place().unwrap_or_else(|| panic!("置き場を作れる"));
    let (head, sha12) = (place.head.as_str(), head12(&place));
    let vessel = place.vessel.display().to_string();
    write_host(&place, &["acc-a"], Some(&vessel));
    let payload = place.vessel.join(PLUGIN_DIR);
    let digest = hooks_at(&payload, "{\"hooks\":{\"PreCompact\":[]}}\n").unwrap_or_else(|| panic!("生成 dir に hooks.json を置ける"));
    assert_ne!(digest, place.digest, "生成 dir の hooks.json は plugin root の fixture と別の本文");
    write_ledger(
        &place,
        "acc-a",
        &[
            LedgerRow { project: "/p/new", scope: Some("project"), install: None, sha: Some(head) },
            LedgerRow { project: "/p/old", scope: Some("project"), install: None, sha: Some(head) },
        ],
    );
    let build = env!("SCRIBE2_BUILD_COMMIT");
    register_anchor(&place, "/p/new", "pn:pn");
    write_record(&place, "pn:pn", &payload, Some(&digest), build);
    register_anchor(&place, "/p/old", "po:po");
    write_record(&place, "po:po", &place.vessel, Some(&digest), build);
    register_anchor(&place, "/p/none", "pu:pu");
    let lines = consumer_lines(&place);
    let payload = payload.display().to_string();
    let want = [
        format!("consumer=/p/new source=launch+install scope=project binary={build} plugin={payload}:{digest} ledger={head} cache=absent head={sha12} behind=unmeasured drift=dual"),
        format!("consumer=/p/none source=launch scope=- binary=unrecorded plugin=unrecorded ledger=- cache=absent head={sha12} behind=unmeasured drift=unrecorded"),
        format!("consumer=/p/old source=launch+install scope=project binary={build} plugin={vessel}:{digest} ledger={head} cache=absent head={sha12} behind=unmeasured drift=plugin"),
    ];
    assert_eq!(lines, want, "読み込み元は生成 dir・旧 root は dual にならない・記録の無い導入先は unrecorded");
    fs::remove_dir_all(&place.dir).ok();
}

/// (f) 壊れた帳簿の口座は `ledger=unreadable` の 1 行（帳簿の path を名指す・末尾）で、他の口座の行は出る。器の key の無い
/// 帳簿・帳簿の無い口座は行 0。doctor は帳簿を書かない（bytes 不変）。
#[test]
fn doctor_consumer_survives_a_broken_ledger() {
    let place = consumer_place().unwrap_or_else(|| panic!("置き場を作れる"));
    write_host(&place, &["acc-broken", "acc-good", "acc-other", "acc-none"], None);
    let broken = write_ledger_text(&place, "acc-broken", "{\"plugins\":");
    write_ledger(&place, "acc-good", &[LedgerRow { project: "/g/one", scope: Some("project"), install: None, sha: None }]);
    write_ledger_text(&place, "acc-other", "{\"version\":2,\"plugins\":{\"other@other\":[{\"projectPath\":\"/o/x\"}]}}");
    let before = fs::read(&broken).unwrap_or_default();
    let lines = consumer_lines(&place);
    let want = [
        "consumer=/g/one source=install scope=project binary=unrecorded plugin=unrecorded ledger=- cache=absent head=undeclared behind=- drift=unrecorded".to_owned(),
        format!(
            "consumer={} source=install scope=- binary=unrecorded plugin=unrecorded ledger=unreadable cache=absent head=undeclared behind=- drift=unrecorded",
            broken.display()
        ),
    ];
    assert_eq!(lines, want, "壊れた帳簿は 1 行・他の行は出る・器の無い帳簿は行 0");
    assert_eq!(fs::read(&broken).unwrap_or_default(), before, "帳簿を書かない");
    assert!(!vessel::account::consumers::ledger_path(&place.state, "acc-none").exists(), "無い口座の帳簿を作らない");
    fs::remove_dir_all(&place.dir).ok();
}

/// (g) `[[vessel]]` が無ければ `head=undeclared`（止めない・帳簿の食い違いは測れない＝`ledger` の語は出ない）。宣言が git の
/// repo でなければ `head=unknown`。宣言が在れば同じ帳簿で `ledger` を名指す。
#[test]
fn doctor_consumer_head_is_undeclared_without_vessel_row() {
    let place = consumer_place().unwrap_or_else(|| panic!("置き場を作れる"));
    write_host(&place, &["acc-a"], None);
    let stale = "2".repeat(40);
    write_ledger(&place, "acc-a", &[LedgerRow { project: "/h/one", scope: Some("project"), install: None, sha: Some(&stale) }]);
    register_anchor(&place, "/h/one", "h:one");
    write_record(&place, "h:one", &place.root, Some(&place.digest), env!("SCRIBE2_BUILD_COMMIT"));
    let plugin = format!("{}:{}", place.root.display(), place.digest);
    let build = env!("SCRIBE2_BUILD_COMMIT");
    let line = |head: &str, behind: &str| {
        format!("consumer=/h/one source=launch+install scope=project binary={build} plugin={plugin} ledger={stale} cache=absent head={head} behind={behind} drift=none")
    };
    assert_eq!(consumer_lines(&place), [line("undeclared", "-")], "[[vessel]] 無し");
    let not_git = place.dir.join("not-a-repo");
    fs::create_dir_all(&not_git).ok();
    write_host(&place, &["acc-a"], Some(&not_git.display().to_string()));
    assert_eq!(consumer_lines(&place), [line("unknown", "unmeasured")], "git の repo でない宣言");
    write_host(&place, &["acc-a"], Some(&place.vessel.display().to_string()));
    let sha12 = head12(&place);
    assert_eq!(
        consumer_lines(&place),
        [line(&sha12, "unmeasured").replace("drift=none", "drift=ledger")],
        "宣言が在れば帳簿の食い違いを測る"
    );
    fs::remove_dir_all(&place.dir).ok();
}

// ---- doctor の consumer 行の `behind=`（consumer-sync.md §15 形 4・接頭辞 `doctor_consumer_behind_`・`s2-07l.408`）----

/// 撃たれた git の argv を 1 行ずつ写してから実 git へ exec する偽 git を `place.dir/bin` に置き、(PATH の値, 写しの path) を返す。
fn logging_git(place: &ConsumerPlace) -> Option<(String, PathBuf)> {
    use std::os::unix::fs::PermissionsExt;
    let real = String::from_utf8_lossy(&Command::new("sh").args(["-c", "command -v git"]).output().ok()?.stdout).trim().to_owned();
    let bin = place.dir.join("bin");
    fs::create_dir_all(&bin).ok()?;
    let log = place.dir.join("git-argv.log");
    let shim = bin.join("git");
    fs::write(&shim, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexec '{real}' \"$@\"\n", log.display())).ok()?;
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).ok()?;
    Some((format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()), log))
}

/// vessel repo の上流の既定 branch（`origin/main` の追跡 ref）を HEAD から `ahead` 個進んだ commit に置く（HEAD は動かさない・
/// fetch の要らない形＝remote を持たない）。
fn upstream_ahead(place: &ConsumerPlace, ahead: usize) -> Option<()> {
    let dir = &place.vessel;
    for step in 0..ahead {
        git_out(dir, &["commit", "-q", "--allow-empty", "-m", &format!("ahead-{step}")])?;
    }
    git_out(dir, &["update-ref", "refs/remotes/origin/main", "HEAD"])?;
    git_out(dir, &["reset", "-q", "--hard", &place.head])?;
    Some(())
}

/// doctor の consumer 行が `head=` の直後に `behind=<n>` を持つ: 上流が 2 個先なら `behind=2`・同じなら `behind=0`・上流の
/// ref が無ければ `unmeasured`（0 と融合しない）。**fetch の argv は 1 本も写らない**（doctor は読むだけ）。base は欄が無い（RED）。
#[test]
fn doctor_consumer_behind_counts_the_upstream_lead_without_fetching() {
    let place = consumer_place().unwrap_or_else(|| panic!("置き場を作れる"));
    let vessel = place.vessel.display().to_string();
    write_host(&place, &["acc-a"], Some(&vessel));
    write_ledger(&place, "acc-a", &[LedgerRow { project: "/b/one", scope: Some("project"), install: None, sha: Some(&place.head) }]);
    let (path, log) = logging_git(&place).unwrap_or_else(|| panic!("偽 git を置ける"));
    let sha12 = head12(&place);
    let line = |behind: &str| {
        format!("consumer=/b/one source=install scope=project binary=unrecorded plugin=unrecorded ledger={} cache=absent head={sha12} behind={behind} drift=unrecorded", place.head)
    };
    assert_eq!(consumer_lines_on(&place, Some(&path)), [line("unmeasured")], "上流の ref が無い周は測れない（0 にしない）");
    upstream_ahead(&place, 2).unwrap_or_else(|| panic!("上流を 2 個先へ置ける"));
    assert_eq!(consumer_lines_on(&place, Some(&path)), [line("2")], "上流が 2 個先");
    assert_eq!(git_out(&place.vessel, &["rev-parse", "HEAD"]).as_deref(), Some(place.head.as_str()), "doctor は HEAD を動かさない");
    git_out(&place.vessel, &["update-ref", "refs/remotes/origin/main", &place.head]).unwrap_or_else(|| panic!("上流を HEAD に揃えられる"));
    assert_eq!(consumer_lines_on(&place, Some(&path)), [line("0")], "上流と同じ");
    let argv = fs::read_to_string(&log).unwrap_or_default();
    assert!(argv.lines().any(|found| found.contains("rev-list --count HEAD..origin/main")), "差は rev-list で数える: {argv}");
    assert!(!argv.lines().any(|found| found.split(' ').any(|word| word == "fetch")), "fetch を撃たない: {argv}");
    fs::remove_dir_all(&place.dir).ok();
}

/// `[[vessel]]` の無い置き場の consumer 行は `behind=-` で、git の argv に差の読みが 1 本も写らない（宣言が無ければ撃たない）。
/// 壊れた帳簿の行も同じ欄を持つ。
#[test]
fn doctor_consumer_behind_is_dash_without_vessel_row() {
    let place = consumer_place().unwrap_or_else(|| panic!("置き場を作れる"));
    write_host(&place, &["acc-a", "acc-broken"], None);
    write_ledger(&place, "acc-a", &[LedgerRow { project: "/b/two", scope: Some("project"), install: None, sha: None }]);
    let broken = write_ledger_text(&place, "acc-broken", "{\"plugins\":");
    let (path, log) = logging_git(&place).unwrap_or_else(|| panic!("偽 git を置ける"));
    let lines = consumer_lines_on(&place, Some(&path));
    let want = [
        "consumer=/b/two source=install scope=project binary=unrecorded plugin=unrecorded ledger=- cache=absent head=undeclared behind=- drift=unrecorded".to_owned(),
        format!(
            "consumer={} source=install scope=- binary=unrecorded plugin=unrecorded ledger=unreadable cache=absent head=undeclared behind=- drift=unrecorded",
            broken.display()
        ),
    ];
    assert_eq!(lines, want, "宣言が無ければ behind=-");
    let argv = fs::read_to_string(&log).unwrap_or_default();
    assert!(!argv.contains("rev-list"), "宣言が無ければ差を読まない: {argv}");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────── host init と doctor の host-template= の行（host-init.md §3・行 a・`s2-07l.612`） ───────────

/// binary の 1 回の結果（rc・stdout・stderr）。
struct HostRun {
    rc: Option<i32>,
    out: String,
    err: String,
}

/// git の global 設定を toy の file `global` に向け（system 設定は読まない）、`cwd` で binary を `args` で撃つ。
fn host_run(global: &Path, cwd: &Path, args: &[&str]) -> Option<HostRun> {
    host_run_on(global, cwd, args, None, None)
}

/// [`host_run`] に PATH（`path`・偽 tmux を先頭に足した値）と argv[0] の字面（`arg0`・`init` の 9 段目が子の program に使う）を
/// 足した形。
fn host_run_on(global: &Path, cwd: &Path, args: &[&str], path: Option<&str>, arg0: Option<&str>) -> Option<HostRun> {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new(env!("CARGO_BIN_EXE_scribe2"));
    command.args(args).current_dir(cwd).env("GIT_CONFIG_GLOBAL", global).env("GIT_CONFIG_NOSYSTEM", "1");
    if let Some(found) = path {
        command.env("PATH", found);
    }
    if let Some(found) = arg0 {
        command.arg0(found);
    }
    let out = command.output().ok()?;
    Some(HostRun {
        rc: out.status.code(),
        out: String::from_utf8_lossy(&out.stdout).into_owned(),
        err: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// toy の global 設定の file が持つ雛形の pointer（無ければ `None`・git の `--file` で読む）。
fn template_in(global: &Path) -> Option<String> {
    let dir = global.parent()?;
    git_out(dir, &["config", "--file", &global.display().to_string(), "--get", &vessel::init::template_key()])
}

/// doctor の出力の 3 行目（骨格の 2 行の直後）。
fn doctor_third(global: &Path, cwd: &Path, extra: &[&str]) -> Option<String> {
    let args: Vec<&str> = ["doctor"].iter().chain(extra).copied().collect();
    let run = host_run(global, cwd, &args)?;
    (run.rc == Some(0)).then_some(())?;
    run.out.lines().nth(2).map(str::to_owned)
}

/// (1)(2)(3) `host init` は雛形の dir（`host.toml` が無い周・在る周）の絶対 path を global 設定へ書いて `written`、同じ値の
/// 2 度目は書かず `unchanged`（file の bytes 不変）。相対の引数も絶対 path で書く。doctor は骨格の 2 行の直後に
/// `host-template=` を出す: 書く前は `absent`・書いた後は path・指す先の `host.toml` が壊れれば `unreadable`（置き場を
/// 渡した周も同じ位置）。base は `host` の verb が無い（RED）。
#[test]
fn host_init_writes_the_absolute_path_once_and_doctor_names_it() {
    let tmp = make_tmp_dir().and_then(TmpDir::canonical).unwrap_or_else(|| panic!("tmp dir を作れる"));
    let global = tmp.join("gitconfig");
    let bare = tmp.join("place");
    fs::create_dir_all(&bare).unwrap_or_else(|e| panic!("雛形の dir を作れる: {e}"));
    let bare_s = bare.display().to_string();
    assert_eq!(doctor_third(&global, &tmp, &[]).as_deref(), Some("host-template=absent"), "書く前は absent");

    let first = host_run(&global, &tmp, &["host", "init", "place"]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!((first.rc, first.err.as_str()), (Some(0), ""), "Absent の雛形を受ける: {}", first.out);
    assert_eq!(first.out, format!("host: init template={bare_s} written\n"), "出力は 1 行・相対の引数も絶対 path");
    assert_eq!(template_in(&global).as_deref(), Some(bare_s.as_str()), "global 設定に絶対 path");
    let before = fs::read(&global).unwrap_or_default();

    let again = host_run(&global, &tmp, &["host", "init", &bare_s]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!((again.rc, again.err.as_str()), (Some(0), ""), "同じ値も rc 0");
    assert_eq!(again.out, format!("host: init template={bare_s} unchanged\n"), "同じ値は unchanged");
    assert_eq!(fs::read(&global).unwrap_or_default(), before, "unchanged の周は書かない");
    assert_eq!(doctor_third(&global, &tmp, &[]), Some(format!("host-template={bare_s}")), "書いた後は path");

    let faced = tmp.join("faced");
    fs::create_dir_all(&faced).unwrap_or_else(|e| panic!("雛形の dir を作れる: {e}"));
    fs::write(faced.join(vessel::rules::HOST_MANIFEST), "schema = 1\n\n[[account]]\nlabel = \"acct\"\n")
        .unwrap_or_else(|e| panic!("host の面を置ける: {e}"));
    let faced_s = faced.display().to_string();
    let second = host_run(&global, &tmp, &["host", "init", &faced_s]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(second.rc, Some(0), "Present の雛形を受ける: {}", second.err);
    assert_eq!(second.out, format!("host: init template={faced_s} written\n"), "値が違えば書く");
    assert_eq!(template_in(&global).as_deref(), Some(faced_s.as_str()), "global 設定は新しい path");
    let socket = tmp.join("no-server-sock").display().to_string();
    let with_state = ["--state-dir", bare_s.as_str(), "--tmux-socket", socket.as_str()];
    assert_eq!(doctor_third(&global, &tmp, &with_state), Some(format!("host-template={faced_s}")), "置き場を渡しても同じ位置");

    fs::write(faced.join(vessel::rules::HOST_MANIFEST), "schema = [\n").unwrap_or_else(|e| panic!("面を壊せる: {e}"));
    assert_eq!(doctor_third(&global, &tmp, &[]).as_deref(), Some("host-template=unreadable"), "壊れた面は unreadable");
    assert_eq!(doctor_third(&global, &tmp, &with_state).as_deref(), Some("host-template=unreadable"), "置き場を渡しても同じ");
}

/// (1) 断る 3 形（引数欠け・dir が無い〔file も dir でない〕・`host.toml` が `Unreadable`）と余分な引数は rc 非 0 で
/// global 設定を 1 byte も書かない（file を作らない・書いた後の bytes も不変）。
#[test]
fn host_init_refuses_without_writing_a_byte() {
    let tmp = make_tmp_dir().and_then(TmpDir::canonical).unwrap_or_else(|| panic!("tmp dir を作れる"));
    let global = tmp.join("gitconfig");
    let broken = tmp.join("broken");
    fs::create_dir_all(&broken).unwrap_or_else(|e| panic!("dir を作れる: {e}"));
    fs::write(broken.join(vessel::rules::HOST_MANIFEST), "schema = [\n").unwrap_or_else(|e| panic!("壊れた面を置ける: {e}"));
    let plain = tmp.join("plain-file");
    fs::write(&plain, "x\n").unwrap_or_else(|e| panic!("file を置ける: {e}"));
    let good = tmp.join("good");
    fs::create_dir_all(&good).unwrap_or_else(|e| panic!("dir を作れる: {e}"));
    let (missing, broken_s, plain_s, good_s) = (
        tmp.join("missing").display().to_string(),
        broken.display().to_string(),
        plain.display().to_string(),
        good.display().to_string(),
    );
    let refusals: [&[&str]; 5] = [
        &["host", "init"],
        &["host", "init", &missing],
        &["host", "init", &plain_s],
        &["host", "init", &broken_s],
        &["host", "init", &good_s, "extra"],
    ];
    for args in refusals {
        let run = host_run(&global, &tmp, args).unwrap_or_else(|| panic!("binary を撃てる"));
        assert!(!matches!(run.rc, Some(0) | None), "断る {args:?}: rc={:?} out={}", run.rc, run.out);
        assert_eq!(run.out, "", "断る周は stdout に書かない {args:?}");
        assert!(!global.exists(), "断る周は global 設定を作らない {args:?}");
    }
    let wrote = host_run(&global, &tmp, &["host", "init", &good_s]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(wrote.rc, Some(0), "受ける雛形は書ける: {}", wrote.err);
    let before = fs::read(&global).unwrap_or_default();
    for args in refusals {
        let run = host_run(&global, &tmp, args).unwrap_or_else(|| panic!("binary を撃てる"));
        assert!(!matches!(run.rc, Some(0) | None), "断る {args:?}");
        assert_eq!(fs::read(&global).unwrap_or_default(), before, "書いた後も断る周は bytes 不変 {args:?}");
    }
    assert_eq!(template_in(&global).as_deref(), Some(good_s.as_str()), "値は受けた雛形のまま");
}

// ─────────── init [ROOT] の 7 段（host-init.md §4・行 b・`s2-07l.613`） ───────────

/// 雛形の面の `[[account-group]]` より前（plugin・launch-arg・口座 2 つ）。
const FACE_FRONT: &str = "schema = 1\n\n[[plugin]]\ndir = \"/opt/plugin\"\n\n[[launch-arg]]\nvalue = \"--verbose\"\n\n\
[[account]]\nlabel = \"a1\"\n\n[[account]]\nlabel = \"a2\"\n\n";
/// 雛形の面の群の表（写さない表・`--group g1` が anchors に ROOT を足す）。
const FACE_GROUP: &str = "[[account-group]]\nname = \"g1\"\nanchors = [\"/elsewhere/x\"]\naccounts = [\"a1\"]\n\n";
/// 雛形の面の群の表より後（vessel・tick）。
const FACE_BACK: &str = "[[vessel]]\nrepo = \"/opt/vessel\"\n\n[[tick]]\nunit-dir = \"/opt/units\"\nbinary = \"/opt/bin/tick\"\n";
/// git の形の宣言の雛形（`Cargo.toml` が無い repo）。
const DECL_GIT: &str = "schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git diff --quiet\"]\nentrance-flip = \"unmeasured\"\n";

/// init の歯の host（toy の global 設定・雛形 `hosts/base`・口座の実 dir・toy の repo `proj`・偽 tmux の PATH）。
struct InitHost {
    tmp: TmpDir,
    global: PathBuf,
    hosts: PathBuf,
    base: PathBuf,
    repo: PathBuf,
    path: String,
}

impl InitHost {
    /// 新しい置き場（`hosts/base-proj`）。
    fn place(&self) -> PathBuf {
        self.hosts.join("base-proj")
    }

    /// `init` を tmp の cwd・偽 tmux の PATH で撃つ（toy の global 設定・9 段目の子は歯の binary 自身）。
    fn init(&self, extra: &[&str]) -> Option<HostRun> {
        self.init_as(None, extra)
    }

    /// [`Self::init`] の argv[0] を `arg0` にした形（9 段目の子の program の出所を差し替える）。
    fn init_as(&self, arg0: Option<&str>, extra: &[&str]) -> Option<HostRun> {
        let repo = self.repo.display().to_string();
        let args: Vec<&str> = ["init", repo.as_str()].iter().chain(extra).copied().collect();
        host_run_on(&self.global, &self.tmp, &args, Some(&self.path), arg0)
    }

    /// 偽 tmux が受けた呼出（1 行 1 回・`-S` の無い既定の socket の形）。
    fn tmux_calls(&self) -> Vec<String> {
        fs::read_to_string(self.tmp.join(INIT_TMUX_ARGS)).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    /// 置き場の登録 row（積んだ順）。
    fn rows(&self) -> Vec<Registration> {
        vessel::fleet::store::read_all(&self.place()).unwrap_or_default().into_iter().filter_map(|event| event.registration).collect()
    }
}

/// init の歯の偽 tmux の呼出の記録（tmp の直下）。
const INIT_TMUX_ARGS: &str = "init-tmux-args";
/// 在れば偽 tmux の session `proj` が在る（`new-session` が作る）。
const INIT_TMUX_LIVE: &str = "init-tmux-live";
/// 在れば偽 tmux はどの呼出も rc 1（tmux を撃てない周）。
const INIT_TMUX_DOWN: &str = "init-tmux-down";

/// 偽 tmux と偽 systemctl を `tmp/init-bin` に置き、その dir を先頭に足した PATH を返す。本物の tmux の server には 1 度も触れない:
/// 偽 tmux は呼出を [`INIT_TMUX_ARGS`] へ写し、`has-session` は [`INIT_TMUX_LIVE`] が在る周だけ rc 0・`new-session` はそれを作り、
/// 起動の口（窓 `orchestrator` が在る・前面は shell・入力欄は空）に答え、`send-keys … Enter` で席の打刻に `SessionStart` を足す
/// （置き場 `place` の `seat/<target>`）。systemctl は rc 0 で何もしない（host の unit に触れない）。
fn init_shims(tmp: &Path, place: &Path) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let bin = tmp.join("init-bin");
    fs::create_dir_all(&bin).ok()?;
    let tmux = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{args}'\n[ -f '{down}' ] && exit 1\n\
         t=''; p=''\nfor a in \"$@\"; do [ \"$p\" = '-t' ] && t=\"$a\"; p=\"$a\"; done\n\
         f=$(printf '%s' \"$t\" | tr ':' '_')\ncase \"$1\" in\n\
         has-session) [ -f '{live}' ] || exit 1;;\nnew-session) : > '{live}';;\n\
         list-windows) echo orchestrator;;\nlist-panes) echo bash;;\ncapture-pane) printf '$ \\n';;\n\
         send-keys) if [ \"$4\" = \"Enter\" ]; then mkdir -p '{seats}/'\"$f\"\n\
         printf '{{\"schema\":1,\"state\":\"idle\",\"event\":\"SessionStart\",\"ts\":%s,\"sid\":\"\"}}\\n' \"$(date +%s)\" \
         >> '{seats}/'\"$f\"'/state.jsonl'; fi;;\n*) exit 1;;\nesac\nexit 0\n",
        args = tmp.join(INIT_TMUX_ARGS).display(),
        down = tmp.join(INIT_TMUX_DOWN).display(),
        live = tmp.join(INIT_TMUX_LIVE).display(),
        seats = place.join("seat").display(),
    );
    for (name, body) in [("tmux", tmux), ("systemctl", "#!/bin/sh\nexit 0\n".to_owned())] {
        fs::write(bin.join(name), body).ok()?;
        fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o755)).ok()?;
    }
    Some(format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()))
}

/// 雛形の面を `face` で置き、口座 a1 を host の設定 dir への symlink・a2 を実 dir にし（credential の file を 1 つずつ）、
/// `host init` で pointer を書き、commit 1 つの repo を作る（`cargo` なら `Cargo.toml` を commit する）。
fn init_host(face: &str, cargo: bool) -> Option<InitHost> {
    let tmp = make_tmp_dir()?.canonical()?;
    let (global, hosts) = (tmp.join("gitconfig"), tmp.join("hosts"));
    let base = hosts.join("base");
    fs::create_dir_all(base.join("accounts").join("a2")).ok()?;
    fs::write(base.join(vessel::rules::HOST_MANIFEST), face).ok()?;
    fs::write(base.join("accounts").join("a2").join(".credentials.json"), "secret-a2\n").ok()?;
    let shared = tmp.join("configs").join("a1");
    fs::create_dir_all(&shared).ok()?;
    fs::write(shared.join(".credentials.json"), "secret-a1\n").ok()?;
    std::os::unix::fs::symlink(&shared, base.join("accounts").join("a1")).ok()?;
    let repo = tmp.join("proj");
    git_repo_at(&repo)?;
    if cargo {
        fs::write(repo.join("Cargo.toml"), "[package]\nname = \"proj\"\n").ok()?;
        git_out(&repo, &["add", "Cargo.toml"])?;
        git_out(&repo, &["commit", "-q", "-m", "cargo"])?;
    }
    let pointed = host_run(&global, &tmp, &["host", "init", &base.display().to_string()])?;
    (pointed.rc == Some(0)).then_some(())?;
    let path = init_shims(&tmp, &hosts.join("base-proj"))?;
    Some(InitHost { tmp, global, hosts, base, repo, path })
}

/// 9 段の出力（`ok|skip|failed:..` を段の順に）と `next=` の 10 行。
fn init_lines(words: [&str; 9], next: &str) -> String {
    let stages = ["state-dir", "host-face", "accounts", "marker", "declaration", "group", "commit", "session", "seat"];
    let mut out: String = stages.iter().zip(words).map(|(stage, word)| format!("init: {stage} {word}\n")).collect();
    out.push_str(&format!("next={next}\n"));
    out
}

/// `path` の下の file と link の相対 path（再帰・link は辿らない・並べて返す）。
fn tree_of(path: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let at = entry.path();
            match fs::symlink_metadata(&at) {
                Ok(meta) if meta.is_dir() => stack.push(at),
                _ => found.push(at.strip_prefix(path).map(|rel| rel.display().to_string()).unwrap_or_default()),
            }
        }
    }
    found.sort();
    found
}

/// 群 `name` の anchors（面が読めない・群が無ければ `None`）。
fn anchors_of(state_dir: &Path, name: &str) -> Option<Vec<String>> {
    match vessel::rules::manifest::HostManifest::read(&state_dir.join(vessel::rules::HOST_MANIFEST)) {
        vessel::rules::manifest::HostManifest::Present(face) => {
            face.groups().iter().find(|group| group.name() == name).map(|group| group.anchors().to_vec())
        }
        _ => None,
    }
}

/// 群の無い新しい置き場の 1 度目の 9 段（口座の実測が無いので 9 段目の子は `no-account` で断る＝`next=fix:seat`）。
const FIRST_UNGROUPED: [&str; 9] = ["ok", "ok", "ok", "ok", "ok", "skip", "ok", "ok", "failed:seat:no-account"];

/// (2)(3)(4)(9) 9 段を順に通す: 置き場は `<雛形>-<repo 名>`・面は `[[account-group]]` だけを除いた写し（tick を含む）・
/// 口座は雛形の symlink の先と実 dir への symlink（credential の file を置き場に写さない）・session は作り・席の子は選定の断りを
/// 写す（子は置き場に何も書かない）。base は `init` の verb が無い（RED）。
#[test]
fn init_repo_runs_nine_stages_into_the_new_place() {
    let host = init_host(&format!("{FACE_FRONT}{FACE_GROUP}{FACE_BACK}"), false).unwrap_or_else(|| panic!("host を組める"));
    let first = host.init(&[]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!((first.rc, first.err.as_str()), (Some(1), ""), "9 段が通り席の段だけ断られる: {}", first.out);
    assert_eq!(first.out, init_lines(FIRST_UNGROUPED, "fix:seat"), "段ごとに 1 行と next=");
    let place = host.place();
    assert!(place.is_dir(), "置き場は雛形の親の下の <雛形>-<repo 名>");
    let face = fs::read_to_string(place.join(vessel::rules::HOST_MANIFEST)).unwrap_or_default();
    assert_eq!(face, format!("{FACE_FRONT}{FACE_BACK}"), "面は群の表だけを除いて 1 字も変えず写す");
    assert_eq!(tree_of(&place), ["accounts/a1", "accounts/a2", "host.toml"], "置き場に在るのは面と口座の link だけ（credential を写さない）");
    let link = |label: &str| fs::read_link(place.join("accounts").join(label)).ok();
    assert_eq!(link("a1"), Some(host.tmp.join("configs").join("a1")), "雛形の symlink の先へ結ぶ");
    assert_eq!(link("a2"), Some(host.base.join("accounts").join("a2")), "雛形の実 dir へ結ぶ");
}

/// (5)(6)(8)(9) marker と local 設定・git の形の宣言・書いた 2 file だけの commit（index に在った他の変更は commit に入らず
/// staged のまま）。`next=fix:seat` の手（口座を名指した `seat launch`）で席が立った後の 2 度目は 9 段が全部 skip で
/// `next=doctor`・面・HEAD・雛形が不変。
#[test]
fn init_repo_commits_only_its_files_and_the_second_run_skips_every_stage() {
    let host = init_host(&format!("{FACE_FRONT}{FACE_GROUP}{FACE_BACK}"), false).unwrap_or_else(|| panic!("host を組める"));
    fs::write(host.repo.join("other"), "staged\n").unwrap_or_else(|e| panic!("他の変更を置ける: {e}"));
    git_out(&host.repo, &["add", "other"]).unwrap_or_else(|| panic!("他の変更を stage できる"));
    let first = host.init(&[]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(first.out, init_lines(FIRST_UNGROUPED, "fix:seat"), "rc={:?} {}", first.rc, first.err);
    let place = host.place();
    let face = fs::read_to_string(place.join(vessel::rules::HOST_MANIFEST)).unwrap_or_default();
    let marker = fs::read_to_string(host.repo.join(".vessel")).unwrap_or_default();
    assert_eq!(marker, format!("name={NAME}\nversion=2\n"), "marker は vessel init と同じ 2 行");
    let config = git_out(&host.repo, &["config", "--local", "--get", &format!("{NAME}.stateDir")]);
    assert_eq!(config, Some(place.display().to_string()), "local 設定は新しい置き場");
    let decl = fs::read_to_string(host.repo.join(".vessel.toml")).unwrap_or_default();
    assert_eq!(decl, DECL_GIT, "Cargo.toml の無い repo は git の形");

    let subject = git_out(&host.repo, &["log", "-1", "--format=%s"]);
    assert_eq!(subject, Some(format!("chore({NAME}): vessel marker and declaration")), "1 commit の題");
    let files = git_out(&host.repo, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files.as_deref(), Some(".vessel\n.vessel.toml"), "commit は書いた 2 file だけ");
    assert_eq!(git_out(&host.repo, &["diff", "--cached", "--name-only"]).as_deref(), Some("other"), "他の staged は触らない");
    let head = git_out(&host.repo, &["rev-parse", "HEAD"]);

    let fixed = host_run_on(&host.global, &host.repo, &["seat", "launch", "--account", "a1"], Some(&host.path), None).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(fixed.rc, Some(0), "口座を名指せば既定形で席が立つ: {} {}", fixed.out, fixed.err);
    let again = host.init(&[]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(again.rc, Some(0), "2 度目は rc 0: {}", again.err);
    assert_eq!(again.out, init_lines(["skip"; 9], "doctor"), "2 度目は 9 段が全部 skip");
    assert_eq!(fs::read_to_string(place.join(vessel::rules::HOST_MANIFEST)).unwrap_or_default(), face, "面は不変");
    assert_eq!(git_out(&host.repo, &["rev-parse", "HEAD"]), head, "2 度目は commit しない");
    assert_eq!(fs::read_to_string(host.base.join(vessel::rules::HOST_MANIFEST)).unwrap_or_default(), format!("{FACE_FRONT}{FACE_GROUP}{FACE_BACK}"), "雛形は不変");
}

/// (6) `Cargo.toml` の在る repo は cargo の形（allowed-commands は cargo と git・common-verify は nextest と clippy・
/// entrance-flip は unmeasured）で、既に在る宣言は 1 字も変えず skip（書いた marker だけを commit）。
#[test]
fn init_repo_declaration_takes_the_cargo_form_and_skips_an_existing_one() {
    let host = init_host(FACE_FRONT, true).unwrap_or_else(|| panic!("host を組める"));
    let first = host.init(&[]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(first.out, init_lines(FIRST_UNGROUPED, "fix:seat"), "rc={:?} {}", first.rc, first.err);
    let decl = fs::read_to_string(host.repo.join(".vessel.toml")).unwrap_or_default();
    assert_eq!(
        decl,
        "schema = 1\nallowed-commands = [\"cargo\", \"git\"]\ncommon-verify = [\"cargo nextest run --workspace --no-tests=fail\", \
         \"cargo clippy --workspace --all-targets -- -D warnings\"]\nentrance-flip = \"unmeasured\"\n",
        "Cargo.toml の在る repo は cargo の形"
    );

    let other = init_host(FACE_FRONT, false).unwrap_or_else(|| panic!("host を組める"));
    fs::write(other.repo.join(".vessel.toml"), "# mine\n").unwrap_or_else(|e| panic!("既存の宣言を置ける: {e}"));
    let run = other.init(&[]).unwrap_or_else(|| panic!("binary を撃てる"));
    let skipped = ["ok", "ok", "ok", "ok", "skip", "skip", "ok", "ok", "failed:seat:no-account"];
    assert_eq!(run.out, init_lines(skipped, "fix:seat"), "宣言だけ skip: {}", run.err);
    assert_eq!(fs::read_to_string(other.repo.join(".vessel.toml")).unwrap_or_default(), "# mine\n", "既存の宣言は不変");
    let files = git_out(&other.repo, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files.as_deref(), Some(".vessel"), "書いた marker だけを commit");
}

/// (7) `--group g1` は雛形と同じ親の下で g1 を宣言する 2 面（雛形・兄弟）の anchors に ROOT を足し、新しい面にも群の行を
/// 写す（宣言しない面は不変）。群の置き場になった ROOT の席は群の今の口座（種 a1）で立ち 9 段が ok。2 度目の `--group` は
/// 9 段が skip。雛形に無い群は failed で名指し、他の段は通る（群に入らない席は選定の断り）。
#[test]
fn init_repo_group_adds_the_root_to_every_declaring_face() {
    let full = format!("{FACE_FRONT}{FACE_GROUP}{FACE_BACK}");
    let host = init_host(&full, false).unwrap_or_else(|| panic!("host を組める"));
    let sibling = host.hosts.join("sibling");
    let sibling_face = "schema = 1\n\n[[account]]\nlabel = \"a1\"\n\n[[account-group]]\nname = \"g1\"\nanchors = [\"/elsewhere/y\"]\naccounts = [\"a1\"]\n";
    let bystander = host.hosts.join("bystander");
    let bystander_face = "schema = 1\n\n[[account]]\nlabel = \"a1\"\n\n[[account-group]]\nname = \"g2\"\nanchors = [\"/elsewhere/z\"]\naccounts = [\"a1\"]\n";
    for (dir, body) in [(&sibling, sibling_face), (&bystander, bystander_face)] {
        fs::create_dir_all(dir).unwrap_or_else(|e| panic!("兄弟の置き場を作れる: {e}"));
        fs::write(dir.join(vessel::rules::HOST_MANIFEST), body).unwrap_or_else(|e| panic!("面を置ける: {e}"));
    }
    let root = host.repo.display().to_string();
    let run = host.init(&["--group", "g1"]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(run.out, init_lines(["ok"; 9], "doctor"), "群の段も席の段も ok: {}", run.err);
    let expected = |first: &str| Some(vec![first.to_owned(), root.clone()]);
    assert_eq!(anchors_of(&host.base, "g1"), expected("/elsewhere/x"), "雛形の面に ROOT");
    assert_eq!(anchors_of(&sibling, "g1"), expected("/elsewhere/y"), "兄弟の面に ROOT");
    assert_eq!(anchors_of(&host.place(), "g1"), expected("/elsewhere/x"), "新しい面にも群の行");
    let edited = full.replace("[\"/elsewhere/x\"]", &format!("[\"/elsewhere/x\", \"{root}\"]"));
    assert_eq!(fs::read_to_string(host.base.join(vessel::rules::HOST_MANIFEST)).unwrap_or_default(), edited, "anchors の行だけが変わる");
    assert_eq!(fs::read_to_string(bystander.join(vessel::rules::HOST_MANIFEST)).unwrap_or_default(), bystander_face, "宣言しない面は不変");

    let again = host.init(&["--group", "g1"]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(again.out, init_lines(["skip"; 9], "doctor"), "2 度目の --group は skip: {}", again.err);

    let other = init_host(&full, false).unwrap_or_else(|| panic!("host を組める"));
    let missing = other.init(&["--group", "nope"]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(missing.rc, Some(1), "failed の段が在れば rc 1");
    let words = ["ok", "ok", "ok", "ok", "ok", "failed:no-group:nope", "ok", "ok", "failed:seat:no-account"];
    assert_eq!(missing.out, init_lines(words, "fix:group"), "雛形に無い群");
    assert_eq!(anchors_of(&other.place(), "g1"), None, "群の行を写さない");
}

/// (7) 1 面でも検査に落ちれば 0 面: 兄弟の面が宣言に無い口座を候補に持つ（足すと loader が断る）周も、読めない面が
/// 同じ親の下に在る周も、どの面も 1 byte も変わらず一時 file も残さない。
#[test]
fn init_repo_group_writes_zero_faces_when_one_face_fails() {
    let full = format!("{FACE_FRONT}{FACE_GROUP}{FACE_BACK}");
    let bad_faces = [
        ("sibling", "schema = 1\n\n[[account]]\nlabel = \"a1\"\n\n[[account-group]]\nname = \"g1\"\nanchors = [\"/elsewhere/y\"]\naccounts = [\"a9\"]\n", "invalid:sibling"),
        ("broken", "schema = [\n", "unreadable:broken"),
    ];
    for (name, body, reason) in bad_faces {
        let host = init_host(&full, false).unwrap_or_else(|| panic!("host を組める"));
        let bad = host.hosts.join(name);
        fs::create_dir_all(&bad).unwrap_or_else(|e| panic!("兄弟の置き場を作れる: {e}"));
        fs::write(bad.join(vessel::rules::HOST_MANIFEST), body).unwrap_or_else(|e| panic!("面を置ける: {e}"));
        let run = host.init(&["--group", "g1"]).unwrap_or_else(|| panic!("binary を撃てる"));
        let group = format!("failed:{reason}");
        let words = ["ok", "ok", "ok", "ok", "ok", group.as_str(), "ok", "ok", "failed:seat:no-account"];
        assert_eq!(run.out, init_lines(words, "fix:group"), "{name}: {}", run.err);
        assert_eq!(fs::read_to_string(host.base.join(vessel::rules::HOST_MANIFEST)).unwrap_or_default(), full, "{name}: 雛形は不変");
        assert_eq!(fs::read_to_string(bad.join(vessel::rules::HOST_MANIFEST)).unwrap_or_default(), body, "{name}: 兄弟は不変");
        assert_eq!(anchors_of(&host.place(), "g1"), None, "{name}: 新しい面に群の行を写さない");
        for dir in [&host.base, &bad, &host.place()] {
            assert!(!dir.join("host.toml.staged").exists(), "{name}: 一時 file を残さない {}", dir.display());
        }
    }
}

/// (4)(5)(9) 失敗の段を名指し、続きの段は止めない: 雛形に dir の無い口座は accounts の段で `failed:no-source:<label>` と
/// 名指して 1 本も結ばず、別の器の marker は marker の段で `failed:by-other:<名>`（marker・設定を書かない＝席の子は置き場を
/// 解けず `defaults-unresolved`）。`next=` は最初の failed の段。
#[test]
fn init_repo_names_the_failed_stage_and_goes_on() {
    let face = format!("{FACE_FRONT}[[account]]\nlabel = \"a3\"\n");
    let host = init_host(&face, false).unwrap_or_else(|| panic!("host を組める"));
    let run = host.init(&[]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(run.rc, Some(1), "failed の段が在れば rc 1");
    let words = ["ok", "ok", "failed:no-source:a3", "ok", "ok", "skip", "ok", "ok", "failed:seat:no-account"];
    assert_eq!(run.out, init_lines(words, "fix:accounts"), "口座の段が最初の failed");
    assert!(!host.place().join("accounts").exists(), "結ぶ先の無い label が在る周は 1 本も結ばない");
    assert!(host.repo.join(".vessel.toml").is_file(), "続きの段は進む");

    let other = init_host(FACE_FRONT, false).unwrap_or_else(|| panic!("host を組める"));
    fs::write(other.repo.join(".vessel"), "name=other\nversion=1\n").unwrap_or_else(|e| panic!("別の器の marker を置ける: {e}"));
    let taken = other.init(&[]).unwrap_or_else(|| panic!("binary を撃てる"));
    let words = ["ok", "ok", "ok", "failed:by-other:other", "ok", "skip", "ok", "ok", "failed:seat:defaults-unresolved"];
    assert_eq!(taken.out, init_lines(words, "fix:marker"), "{}", taken.err);
    assert_eq!(fs::read_to_string(other.repo.join(".vessel")).unwrap_or_default(), "name=other\nversion=1\n", "別の器の marker は不変");
    assert_eq!(git_out(&other.repo, &["config", "--local", "--get", &format!("{NAME}.stateDir")]), None, "設定を書かない");
    let files = git_out(&other.repo, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files.as_deref(), Some(".vessel.toml"), "書いた宣言だけを commit");
}

/// (1) ROOT が git の repo でない周と host-template が無い周は 1 段目の前に断り、置き場も marker も宣言も設定も書かない。
#[test]
fn init_repo_refuses_before_the_first_stage_without_writing() {
    let host = init_host(FACE_FRONT, false).unwrap_or_else(|| panic!("host を組める"));
    let plain = host.tmp.join("plain");
    fs::create_dir_all(&plain).unwrap_or_else(|e| panic!("非 repo の dir を作れる: {e}"));
    let plain_s = plain.display().to_string();
    let run = host_run(&host.global, &host.tmp, &["init", &plain_s]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert!(!matches!(run.rc, Some(0) | None), "非 repo は断る: {}", run.out);
    assert_eq!(run.out, "", "断る周は段の行を出さない");
    assert!(!host.hosts.join("base-plain").exists(), "置き場を作らない");
    assert!(!plain.join(".vessel").exists() && !plain.join(".vessel.toml").exists(), "marker も宣言も書かない");

    let fresh = host.tmp.join("fresh-gitconfig");
    let repo = host.repo.display().to_string();
    let bare = host_run(&fresh, &host.tmp, &["init", &repo]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert!(!matches!(bare.rc, Some(0) | None), "pointer が無い周は断る: {}", bare.out);
    assert_eq!(bare.out, "", "断る周は段の行を出さない");
    assert!(!host.place().exists(), "置き場を作らない");
    assert!(!host.repo.join(".vessel").exists() && !host.repo.join(".vessel.toml").exists(), "marker も宣言も書かない");
    assert_eq!(git_out(&host.repo, &["config", "--local", "--get", &format!("{NAME}.stateDir")]), None, "設定を書かない");
}

// ─────────── init の tmux の session と席の 2 段（host-init.md §5・行 c・`s2-07l.614`・接頭辞 `init_seat_`） ───────────
//
// tmux は [`init_shims`] の偽 tmux だけを撃つ（本物の server に触れない）。9 段目の子を数えるときは argv[0] を包みの script に
// 差し替える: 包みは `$0`・引数・cwd を 1 行で記録してから歯の binary へ exec する（子は本物の `seat launch`）。

/// 群を持つ雛形の面（tick の無い形＝席が立った周に host の unit を触らない）。
fn grouped_face() -> String {
    format!("{FACE_FRONT}{FACE_GROUP}")
}

/// 9 段目の子の包み（`$0|引数|cwd` を `log` に 1 行足して歯の binary へ exec する）を `at` に置く。
fn child_wrapper(at: &Path, log: &Path) -> Option<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(at.parent()?).ok()?;
    let body = format!(
        "#!/bin/sh\nprintf '%s|%s|%s\\n' \"$0\" \"$*\" \"$(pwd)\" >> '{log}'\nexec '{bin}' \"$@\"\n",
        log = log.display(),
        bin = env!("CARGO_BIN_EXE_scribe2"),
    );
    fs::write(at, body).ok()?;
    fs::set_permissions(at, fs::Permissions::from_mode(0o755)).ok()
}

/// 包みの記録（1 行 1 回の子）。
fn child_calls(log: &Path) -> Vec<String> {
    fs::read_to_string(log).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// (2) 8 段目: session `proj` が無ければ `has-session -t =proj` の後に `new-session -d -s proj -n orchestrator -c <ROOT>` を 1 回
/// 撃って ok、2 度目は在るので skip（new-session は増えない）。初めから在る host では skip で new-session は 0 回。
#[test]
fn init_seat_opens_the_session_once_and_skips_it_when_present() {
    let host = init_host(FACE_FRONT, false).unwrap_or_else(|| panic!("host を組める"));
    let first = host.init(&[]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(first.out, init_lines(FIRST_UNGROUPED, "fix:seat"), "session は ok: {}", first.err);
    let made = format!("new-session -d -s proj -n orchestrator -c {}", host.repo.display());
    let calls = host.tmux_calls();
    assert_eq!(calls.iter().filter(|found| found.starts_with("new-session")).collect::<Vec<_>>(), vec![&made], "new-session は 1 回・この形");
    let asked = calls.iter().position(|found| found == "has-session -t =proj");
    assert!(asked.is_some_and(|at| calls.get(at.saturating_add(1)) == Some(&made)), "在るかを確かめてから作る: {calls:?}");
    let again = host.init(&[]).unwrap_or_else(|| panic!("binary を撃てる"));
    let words = ["skip", "skip", "skip", "skip", "skip", "skip", "skip", "skip", "failed:seat:no-account"];
    assert_eq!(again.out, init_lines(words, "fix:seat"), "2 度目の session は skip: {}", again.err);
    assert_eq!(host.tmux_calls().iter().filter(|found| found.starts_with("new-session")).count(), 1, "2 度目は作らない");

    let live = init_host(FACE_FRONT, false).unwrap_or_else(|| panic!("host を組める"));
    fs::write(live.tmp.join(INIT_TMUX_LIVE), "").unwrap_or_else(|e| panic!("session を在らせる: {e}"));
    let run = live.init(&[]).unwrap_or_else(|| panic!("binary を撃てる"));
    let words = ["ok", "ok", "ok", "ok", "ok", "skip", "ok", "skip", "failed:seat:no-account"];
    assert_eq!(run.out, init_lines(words, "fix:seat"), "在る session は skip: {}", run.err);
    assert!(!live.tmux_calls().iter().any(|found| found.starts_with("new-session")), "在る周は作らない");
}

/// (3)(5) 9 段目: 群の置き場になった ROOT の席は、子 `seat launch`（引数無し・cwd = ROOT・1 回）が群の今の口座 a1 で起こして
/// ok（登録 row は役割 orchestrator・anchor = ROOT・target `proj:orchestrator`）・9 段が ok で `next=doctor`。row が在る 2 度目は
/// 子を撃たず 9 段が skip。
#[test]
fn init_seat_launches_the_default_form_once_and_skips_a_registered_row() {
    let host = init_host(&grouped_face(), false).unwrap_or_else(|| panic!("host を組める"));
    let (wrapper, log) = (host.tmp.join("wrap").join(NAME), host.tmp.join("child-log"));
    child_wrapper(&wrapper, &log).unwrap_or_else(|| panic!("包みを置ける"));
    let arg0 = wrapper.display().to_string();
    let first = host.init_as(Some(&arg0), &["--group", "g1"]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!((first.rc, first.out.as_str()), (Some(0), init_lines(["ok"; 9], "doctor").as_str()), "{}", first.err);
    assert_eq!(child_calls(&log), vec![format!("{arg0}|seat launch|{}", host.repo.display())], "子は既定形を ROOT で 1 回");
    let rows: Vec<_> = host.rows().into_iter().map(|row| (row.role, row.anchor, row.target, row.account)).collect();
    let want = (Role::Orchestrator, host.repo.display().to_string(), "proj:orchestrator".to_owned(), "a1".to_owned());
    assert_eq!(rows, vec![want], "登録 row は 1 件");
    let again = host.init_as(Some(&arg0), &["--group", "g1"]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!((again.rc, again.out.as_str()), (Some(0), init_lines(["skip"; 9], "doctor").as_str()), "{}", again.err);
    assert_eq!(child_calls(&log).len(), 1, "row が在れば子を撃たない");
}

/// (3)(4) 子が断った周は子の断りの語をそのまま `failed:seat:<語>` に写して `next=fix:seat`・rc 1（群の外の置き場は選定で
/// `no-account`）。登録 row が在る周は子を撃たず skip で `next=doctor`。
#[test]
fn init_seat_copies_the_child_refusal_word_and_skips_after_registration() {
    let host = init_host(FACE_FRONT, false).unwrap_or_else(|| panic!("host を組める"));
    let (wrapper, log) = (host.tmp.join("wrap").join(NAME), host.tmp.join("child-log"));
    child_wrapper(&wrapper, &log).unwrap_or_else(|| panic!("包みを置ける"));
    let arg0 = wrapper.display().to_string();
    let first = host.init_as(Some(&arg0), &[]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!((first.rc, first.out.as_str()), (Some(1), init_lines(FIRST_UNGROUPED, "fix:seat").as_str()), "{}", first.err);
    assert_eq!(child_calls(&log).len(), 1, "子を 1 回撃つ");
    let row = Registration {
        role: Role::Orchestrator,
        anchor: host.repo.display().to_string(),
        target: "proj:orchestrator".to_owned(),
        sid: None,
        account: "a1".to_owned(),
        launch: String::new(),
        model: None,
    };
    vessel::seat::role::register(&host.place(), row).unwrap_or_else(|e| panic!("row を積める: {e:?}"));
    let again = host.init_as(Some(&arg0), &[]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!((again.rc, again.out.as_str()), (Some(0), init_lines(["skip"; 9], "doctor").as_str()), "{}", again.err);
    assert_eq!(child_calls(&log).len(), 1, "row が在れば子を撃たない");
}

/// (2)(3) tmux を撃てない周は 8 段目が `failed:tmux` で、9 段目も行を出して子を撃つ（群の口座は解けるので子は session の
/// 無さで `session-missing` と断る・row を書かない）。`next=` は最初の failed の `fix:session`・rc 1。
#[test]
fn init_seat_failed_tmux_still_fires_the_seat_stage() {
    let host = init_host(&grouped_face(), false).unwrap_or_else(|| panic!("host を組める"));
    fs::write(host.tmp.join(INIT_TMUX_DOWN), "").unwrap_or_else(|e| panic!("tmux を落とせる: {e}"));
    let run = host.init(&["--group", "g1"]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(run.rc, Some(1), "failed の段が在れば rc 1");
    let words = ["ok", "ok", "ok", "ok", "ok", "ok", "ok", "failed:tmux", "failed:seat:session-missing"];
    assert_eq!(run.out, init_lines(words, "fix:session"), "{}", run.err);
    assert!(host.rows().is_empty(), "断った子は row を書かない");
}

/// (6) argv[0] が `/` を含む相対 path の周は init の cwd で解いてから子を撃つ: ROOT の下に同じ相対 path の囮を置いても、子は
/// init の cwd の下の包み（絶対 path の `$0`）で、囮は撃たれない。子は歯の binary なので選定の断りを写す。
#[test]
fn init_seat_relative_program_points_the_child_to_the_same_binary() {
    use std::os::unix::fs::PermissionsExt;
    let host = init_host(FACE_FRONT, false).unwrap_or_else(|| panic!("host を組める"));
    let (wrapper, log) = (host.tmp.join("rel").join(NAME), host.tmp.join("child-log"));
    child_wrapper(&wrapper, &log).unwrap_or_else(|| panic!("包みを置ける"));
    let (decoy, decoy_log) = (host.repo.join("rel").join(NAME), host.tmp.join("decoy-log"));
    fs::create_dir_all(host.repo.join("rel")).unwrap_or_else(|e| panic!("囮の dir を作れる: {e}"));
    fs::write(&decoy, format!("#!/bin/sh\necho decoy >> '{}'\nexit 0\n", decoy_log.display())).unwrap_or_else(|e| panic!("囮を置ける: {e}"));
    fs::set_permissions(&decoy, fs::Permissions::from_mode(0o755)).unwrap_or_else(|e| panic!("囮を撃てる形にする: {e}"));
    let relative = format!("rel/{NAME}");
    let run = host.init_as(Some(&relative), &[]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert_eq!(run.out, init_lines(FIRST_UNGROUPED, "fix:seat"), "子は歯の binary: {}", run.err);
    assert_eq!(child_calls(&log), vec![format!("{}|seat launch|{}", wrapper.display(), host.repo.display())], "子は init の cwd の下の包み");
    assert!(!decoy_log.exists(), "ROOT の下の囮は撃たれない");
}

// ─────────── doctor の init= の行（host-init.md §6・行 d・`s2-07l.615`） ───────────

/// 6 項目が全部そろった toy（`init` の 7 段を通した置き場・偽 tmux の session `proj`・登録 row）。tmux は立てない: PATH の
/// 先頭の偽 tmux が `has-session -t =proj` にだけ（`-S <toy の socket>` か socket 無し＝既定の形で）rc 0 を返す。
struct ReadyToy {
    host: InitHost,
    socket: String,
    path: String,
}

impl ReadyToy {
    /// doctor を偽 tmux の PATH・`cwd` で `--state-dir <置き場>` と `extra` を付けて撃ち、`init=` の行（4 行目）を返す。
    fn init_line(&self, global: &Path, cwd: &Path, extra: &[&str]) -> Option<String> {
        let place = self.host.place().display().to_string();
        let out = Command::new(env!("CARGO_BIN_EXE_scribe2"))
            .args(["doctor", "--state-dir", place.as_str()])
            .args(extra)
            .current_dir(cwd)
            .env("GIT_CONFIG_GLOBAL", global)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("PATH", &self.path)
            .output()
            .ok()?;
        (out.status.code() == Some(0)).then_some(())?;
        String::from_utf8_lossy(&out.stdout).lines().nth(3).map(str::to_owned)
    }

    /// repo の cwd・toy の global 設定・toy の socket で撃った `init=` の行。
    fn line(&self) -> Option<String> {
        self.init_line(&self.host.global, &self.host.repo, &["--tmux-socket", &self.socket])
    }
}

/// 偽 tmux を `dir/bin/tmux` に置き、その dir を先頭に足した PATH の値を返す（`session` が偽なら常に rc 1＝session が無い）。
fn stub_session_path(dir: &Path, socket: &str, session: bool) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = dir.join("bin");
    fs::create_dir_all(&bin_dir).ok()?;
    let stub = bin_dir.join("tmux");
    let body = if session {
        format!("#!/bin/sh\ncase \"$*\" in\n  \"-S {socket} has-session -t =proj\" | \"has-session -t =proj\") exit 0 ;;\nesac\nexit 1\n")
    } else {
        "#!/bin/sh\nexit 1\n".to_owned()
    };
    fs::write(&stub, body).ok()?;
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).ok()?;
    Some(format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default()))
}

/// `registered` が真なら置き場に役割 orchestrator・anchor = repo の登録 row を積み、`session` が真なら偽 tmux に session
/// `proj` を持たせる（6 項目のうち面・口座・marker・宣言は `init` の 7 段が置く・席の段は選定の断りで row を書かない）。
fn ready_toy(session: bool, registered: bool) -> Option<ReadyToy> {
    let host = init_host(FACE_FRONT, false)?;
    let run = host.init(&[])?;
    (run.out == init_lines(FIRST_UNGROUPED, "fix:seat")).then_some(())?;
    let socket = host.tmp.join("sock").display().to_string();
    let path = stub_session_path(&host.tmp, &socket, session)?;
    if registered {
        let row = Registration {
            role: Role::Orchestrator,
            anchor: host.repo.display().to_string(),
            target: "proj:orchestrator".to_owned(),
            sid: None,
            account: "a1".to_owned(),
            launch: String::new(),
            model: None,
        };
        vessel::seat::role::register(&host.place(), row).ok()?;
    }
    Some(ReadyToy { host, socket, path })
}

/// (1)(2)(3) 6 項目がそろった toy は `init=ok next=-` を `host-template=` の直後に出し、`--repo` は cwd に勝つ（repo でない
/// cwd から `--repo` で名指しても同じ）。置き場を渡さない doctor は `init=` の行を出さない。base は行が無い（RED）。
#[test]
fn doctor_init_names_nothing_when_every_item_is_in_place() {
    let toy = ready_toy(true, true).unwrap_or_else(|| panic!("toy を組める"));
    assert_eq!(toy.line().as_deref(), Some("init=ok next=-"), "欠落 0");
    let plain = toy.host.tmp.join("plain");
    fs::create_dir_all(&plain).unwrap_or_else(|e| panic!("非 repo の dir を作れる: {e}"));
    let repo = toy.host.repo.display().to_string();
    assert_eq!(toy.init_line(&toy.host.global, &plain, &["--repo", &repo]).as_deref(), Some("init=ok next=-"), "--repo が cwd に勝つ");
    let bare = host_run(&toy.host.global, &toy.host.repo, &["doctor"]).unwrap_or_else(|| panic!("binary を撃てる"));
    assert!(!bare.out.lines().any(|line| line.starts_with("init=")), "置き場を渡さない周は出さない: {}", bare.out);
}

/// (2)(3) 6 項目それぞれ 1 つだけ欠けた toy は、その項目だけを名指し `next=` を固定の対応で 1 語置く（registration だけ
/// seat-launch・他は init）。面が無い周は口座の dir が無くても accounts を数えない。宣言は worktree に在っても HEAD に無ければ欠落。
#[test]
fn doctor_init_names_each_single_missing_item_with_its_next() {
    let face = ready_toy(true, true).unwrap_or_else(|| panic!("toy を組める"));
    fs::remove_file(face.host.place().join(vessel::rules::HOST_MANIFEST)).unwrap_or_else(|e| panic!("面を外せる: {e}"));
    assert_eq!(face.line().as_deref(), Some("init=missing:host-face next=init"), "面が無い");
    fs::remove_dir_all(face.host.place().join("accounts")).unwrap_or_else(|e| panic!("口座の link を外せる: {e}"));
    assert_eq!(face.line().as_deref(), Some("init=missing:host-face next=init"), "面が無い周は accounts を数えない");

    let accounts = ready_toy(true, true).unwrap_or_else(|| panic!("toy を組める"));
    fs::remove_file(accounts.host.place().join("accounts").join("a2")).unwrap_or_else(|e| panic!("口座の link を外せる: {e}"));
    assert_eq!(accounts.line().as_deref(), Some("init=missing:accounts next=init"), "口座の dir が 1 つ無い");

    let marker = ready_toy(true, true).unwrap_or_else(|| panic!("toy を組める"));
    fs::write(marker.host.repo.join(".vessel"), "name=other\nversion=1\n").unwrap_or_else(|e| panic!("marker を差し替えられる: {e}"));
    assert_eq!(marker.line().as_deref(), Some("init=missing:marker next=init"), "marker が ByMe でない");

    let declaration = ready_toy(true, true).unwrap_or_else(|| panic!("toy を組める"));
    git_out(&declaration.host.repo, &["rm", "-q", "--cached", ".vessel.toml"]).unwrap_or_else(|| panic!("宣言を index から外せる"));
    git_out(&declaration.host.repo, &["commit", "-q", "-m", "drop"]).unwrap_or_else(|| panic!("commit できる"));
    assert!(declaration.host.repo.join(".vessel.toml").is_file(), "worktree には残る");
    assert_eq!(declaration.line().as_deref(), Some("init=missing:declaration next=init"), "宣言が HEAD に無い");

    let session = ready_toy(false, true).unwrap_or_else(|| panic!("toy を組める"));
    assert_eq!(session.line().as_deref(), Some("init=missing:session next=init"), "session <repo 名> が無い");
    let elsewhere = ready_toy(true, true).unwrap_or_else(|| panic!("toy を組める"));
    let other = elsewhere.host.tmp.join("other-sock").display().to_string();
    assert_eq!(
        elsewhere.init_line(&elsewhere.host.global, &elsewhere.host.repo, &["--tmux-socket", &other]).as_deref(),
        Some("init=missing:session next=init"),
        "session は --tmux-socket の socket で測る"
    );
    assert_eq!(
        elsewhere.init_line(&elsewhere.host.global, &elsewhere.host.repo, &[]).as_deref(),
        Some("init=ok next=-"),
        "--tmux-socket が無ければ既定の socket"
    );

    let registration = ready_toy(true, false).unwrap_or_else(|| panic!("toy を組める"));
    assert_eq!(registration.line().as_deref(), Some("init=missing:registration next=seat-launch"), "登録 row が無い");
}

/// (3) 欠落が在り雛形の pointer が無い周は、最初の欠落に依らず `next=host-init`（欠落 0 は `-` のまま）。
#[test]
fn doctor_init_points_to_host_init_without_a_template() {
    let toy = ready_toy(true, false).unwrap_or_else(|| panic!("toy を組める"));
    let fresh = toy.host.tmp.join("fresh-gitconfig");
    assert_eq!(toy.init_line(&fresh, &toy.host.repo, &[]).as_deref(), Some("init=missing:registration next=host-init"), "pointer が無い");
    fs::remove_file(toy.host.repo.join(".vessel")).unwrap_or_else(|e| panic!("marker を外せる: {e}"));
    assert_eq!(
        toy.init_line(&fresh, &toy.host.repo, &[]).as_deref(),
        Some("init=missing:marker,registration next=host-init"),
        "欠落は段の順に並ぶ"
    );
    let whole = ready_toy(true, true).unwrap_or_else(|| panic!("toy を組める"));
    let fresh = whole.host.tmp.join("fresh-gitconfig");
    assert_eq!(whole.init_line(&fresh, &whole.host.repo, &[]).as_deref(), Some("init=ok next=-"), "欠落 0 は pointer に依らない");
}

/// (1) ROOT が git の repo でない周（cwd も `--repo` も）は `init=unmeasured:no-repo next=-`。他の行は出たまま。
#[test]
fn doctor_init_is_unmeasured_outside_a_repo() {
    let toy = ready_toy(true, true).unwrap_or_else(|| panic!("toy を組める"));
    let plain = toy.host.tmp.join("plain");
    fs::create_dir_all(&plain).unwrap_or_else(|e| panic!("非 repo の dir を作れる: {e}"));
    assert_eq!(toy.init_line(&toy.host.global, &plain, &[]).as_deref(), Some("init=unmeasured:no-repo next=-"), "repo でない cwd");
    let plain_s = plain.display().to_string();
    assert_eq!(
        toy.init_line(&toy.host.global, &toy.host.repo, &["--repo", &plain_s]).as_deref(),
        Some("init=unmeasured:no-repo next=-"),
        "repo でない --repo"
    );
}
