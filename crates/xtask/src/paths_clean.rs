//! paths-clean（tracked file の本文に private path 形が残っていないこと）。
//!
//! 母集団は **index**（`git ls-files -s`）で、本文は通常 file なら作業木・symlink なら
//! index の blob から読む。`check.rs` から分けたのは行数のためだけではない——測定 1 本が
//! 「母集団の作り方」「本文の取り方」「免除の絞り方」の 3 つを持ち、どれも
//! **測れなかったを異常なしへ落とさない**極性を要求するからである。

use crate::check::{failed, Layout, Measured, PATHS_CLEAN_SKIP};
use crate::limits::PRIVATE_PATH_MARKS;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// index が symlink に付ける mode（`git ls-files -s` の 1 列目）。
const SYMLINK_MODE: &str = "120000";

/// paths-clean の母集団（tracked file の repo 相対 path）と root 一致判定の結果。
/// index が持つ tracked file 1 件。
///
/// **mode と oid を運ぶ**のは symlink のためである。symlink の「中身」は作業木では
/// 追跡先の file であって link target 文字列ではないので、作業木から読むと
/// private path 形の target（`git ls-files` には出るが本文としては読めない）が
/// 素通りする。index の blob を読めば target 文字列そのものが得られる。
struct TrackedFile {
    /// repo 相対 path。
    rel: String,
    /// index の mode（symlink は 120000）。
    mode: String,
    /// blob の oid。
    oid: String,
}

impl TrackedFile {
    /// index の mode が symlink か。
    fn is_symlink(&self) -> bool {
        self.mode == SYMLINK_MODE
    }
}

/// paths-clean の母集団を測れたかどうか。
enum Tracked {
    /// `<root>` が repo root であり tracked file を列挙できた。
    Listed(Vec<TrackedFile>),
    /// `<root>` が repo root でない（flip-check の base tree はこの枝に落ちる）。
    NotRepoRoot,
    /// 測れなかった（fail-closed で `n/a` へ落とさない）。
    Unmeasurable(String),
}

/// `git -C <dir> <args...>` を撃ち rc 0 のときだけ stdout を返す。
fn git_stdout(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let shown = args.join(" ");
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|err| format!("git {shown} を起動できない: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {shown} が rc≠0: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}

/// path を canonicalize する。失敗は path と io error を逐語で載せて fail-closed。
fn canonical(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path)
        .map_err(|err| format!("{} を canonicalize できない: {err}", path.display()))
}

/// `<root>` が git repo の toplevel そのものか。
///
/// 素の [`PathBuf`] 比較では `CARGO_MANIFEST_DIR/../..` 形の root が字面で一致せず
/// paths-clean が恒久 `n/a` に化けるので、両側を canonicalize して比べる。
fn root_is_repo_root(root: &Path) -> Result<bool, String> {
    let raw = git_stdout(root, &["rev-parse", "--show-toplevel"])?;
    let shown = String::from_utf8_lossy(&raw).trim().to_owned();
    if shown.is_empty() {
        return Err("git rev-parse --show-toplevel が空を返した".to_owned());
    }
    Ok(canonical(Path::new(&shown))? == canonical(root)?)
}

/// NUL 区切りの出力を path の列へ分ける。
fn split_nul(raw: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(raw)
        .split('\0')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `git ls-files -s -z` の 1 件（`<mode> <oid> <stage>\t<path>`）を読む。
///
/// 形が合わない行は**落とさず捨てる**のではなく `None` を返し、呼び手が
/// 「列挙できなかった」へ倒す——母集団の欠けを静かな 0 件にしないためである。
fn parse_ls_entry(part: &str) -> Option<TrackedFile> {
    let (meta, rel) = part.split_once('\t')?;
    let mut fields = meta.split_whitespace();
    let mode = fields.next()?.to_owned();
    let oid = fields.next()?.to_owned();
    fields.next()?;
    Some(TrackedFile {
        rel: rel.to_owned(),
        mode,
        oid,
    })
}

/// paths-clean の母集団を data 化して固定する（cwd を直読みしない）。
fn tracked_files(root: &Path) -> Tracked {
    match root_is_repo_root(root) {
        Err(reason) => Tracked::Unmeasurable(reason),
        Ok(false) => Tracked::NotRepoRoot,
        Ok(true) => match git_stdout(root, &["ls-files", "-s", "-z"]) {
            Err(reason) => Tracked::Unmeasurable(format!("tracked file を列挙できない: {reason}")),
            Ok(raw) => listed_from(&split_nul(&raw)),
        },
    }
}

/// `git ls-files -s -z` の各件を母集団へ畳む。
///
/// **1 件でも形が読めなければ全体を [`Tracked::Unmeasurable`] へ倒す**。読めた分だけで
/// 走査すると、母集団の欠けが「違反 0 件」として静かに合格するからである。件数を
/// 名指すのは、何件落ちたかを事後に数えられるようにするため。
fn listed_from(parts: &[String]) -> Tracked {
    let listed: Vec<TrackedFile> = parts.iter().filter_map(|part| parse_ls_entry(part)).collect();
    if listed.len() == parts.len() {
        Tracked::Listed(listed)
    } else {
        Tracked::Unmeasurable(format!(
            "tracked file の {} 件中 {} 件しか読めない（ls-files -s の形が違う）",
            parts.len(),
            listed.len()
        ))
    }
}

/// tracked file の本文に private path 形が残っていないこと（paths-clean）。
pub(crate) fn measure(layout: &Layout) -> Measured {
    match tracked_files(&layout.root) {
        Tracked::Unmeasurable(reason) => failed("paths-clean", &reason),
        Tracked::NotRepoRoot => Measured {
            fact: "paths-clean=n/a(not-a-repo-root)".to_owned(),
            violations: Vec::new(),
        },
        Tracked::Listed(listed) if listed.is_empty() => {
            failed("paths-clean", "tracked file が 0 件である")
        }
        Tracked::Listed(listed) => scan_private_paths(&layout.root, &listed),
    }
}

/// tracked file を 1 本ずつ走査する。
///
/// 本文は **byte で読み byte で検索する**。`String` へ通すと非 UTF-8 の 1 byte を混ぜる
/// だけで file 全体が読めなくなり、needle が素通りするからである。読めない file
/// （作業木から消えている・permission が無い等）は無言で skip せず違反として数える。
///
/// **symlink は index の blob を読む**（作業木から読むと追跡先の中身になり、link target
/// 文字列そのものが母集団から落ちる）。通常 file は従来どおり作業木から読む——tracked
/// でも作業木側が書き換わっている周に、index の古い blob で合格させないためである。
fn scan_private_paths(root: &Path, listed: &[TrackedFile]) -> Measured {
    let mut violations = Vec::new();
    let mut scanned = 0;
    for file in listed {
        let rel = &file.rel;
        match body_of(root, file) {
            Err(reason) => violations.push(format!(
                "paths-clean: {rel} を読めない: {reason}（読めない tracked file は違反である）"
            )),
            Ok(bytes) => {
                scanned += 1;
                violations.extend(
                    violating_lines(rel, &bytes)
                        .into_iter()
                        .map(|line| format!("paths-clean: {rel}:{line} に private path 形が在る")),
                );
            }
        }
    }
    Measured {
        fact: format!("paths-clean={scanned}"),
        violations,
    }
}

/// 走査する本文を取る。symlink だけ index の blob（= link target 文字列）を読む。
fn body_of(root: &Path, file: &TrackedFile) -> Result<Vec<u8>, String> {
    if file.is_symlink() {
        return git_stdout(root, &["cat-file", "blob", &file.oid]);
    }
    fs::read(root.join(&file.rel)).map_err(|err| err.to_string())
}

/// 違反として数える行番号。免除 file では **コメント行だけ**を除く。
///
/// file 全文の免除は「その file の中でだけ private path を書き放題」という穴になる。
/// 免除の理由は **道具が生成した説明コメントに例として private path 形が載る**ことなので、
/// 免除もコメント行に限る（設定値として書いた private path は違反のままにする）。
fn violating_lines(rel: &str, bytes: &[u8]) -> Vec<usize> {
    let lines = private_path_lines(bytes);
    if rel != PATHS_CLEAN_SKIP {
        return lines;
    }
    lines
        .into_iter()
        .filter(|line| !is_comment_line(bytes, *line))
        .collect()
}

/// 1 起点の行番号の行がコメント行か（行頭の空白の後の 1 文字が `#`）。
fn is_comment_line(bytes: &[u8], line: usize) -> bool {
    bytes
        .split(|byte| *byte == b'\n')
        .nth(line.saturating_sub(1))
        .and_then(|body| {
            let at = body.iter().position(|byte| !byte.is_ascii_whitespace())?;
            body.get(at).copied()
        })
        .is_some_and(|head| head == b'#')
}

/// private path 形を含む行の番号を昇順・重複なしで返す（行中のどこに在っても違反）。
fn private_path_lines(bytes: &[u8]) -> Vec<usize> {
    let mut lines = BTreeSet::new();
    for mark in PRIVATE_PATH_MARKS {
        for at in find_all(bytes, mark.as_bytes()) {
            lines.insert(line_of(bytes, at));
        }
    }
    lines.into_iter().collect()
}

/// `needle` の現れる byte offset を昇順で返す（std だけの素朴走査）。
fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return Vec::new();
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at)
        .collect()
}

/// byte offset を 1 起点の行番号にする。
fn line_of(bytes: &[u8], at: usize) -> usize {
    bytes.iter().take(at).filter(|byte| **byte == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    // flip-check: retroactive s2-07l.33
    // 下の 2 本は **後から足した歯**である（測定そのものは s2-07l.18 までに land 済みで、
    // base でも緑になる）。RED→GREEN で非空虚性を示せないので、契約の done (iii) が
    // 求める変異 proof（門を外す変異・違反を数えない変異が rc 100 で落ちる）で担保する。
    // flip-check: retroactive s2-07l.35
    // s2-07l.35 が足す 5 本も **後から足した歯**である（meta 3 列の parse・0 件の門・
    // `n/a` の枝・needle 境界はどれも s2-07l.18 までに land 済みで base でも緑になる）。
    // 非空虚性は契約の変異 proof 5 本（当てると各 rc 100 で落ちる）で担保する。
    use super::{
        find_all, listed_from, measure, parse_ls_entry, scan_private_paths, Layout, Tracked,
        TrackedFile,
    };
    use crate::limits::PRIVATE_PATH_MARKS;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// index が通常 file に付ける mode（`git ls-files -s` の 1 列目）。
    const REGULAR_MODE: &str = "100644";
    /// 本 mod の歯は blob を読まない（symlink を作らない）ので oid は形だけでよい。
    const DUMMY_OID: &str = "0000000000000000000000000000000000000000";

    /// `git ls-files -s -z` の 1 件（`<mode> <oid> <stage>\t<path>`）を組む。
    fn entry(rel: &str) -> String {
        format!("{REGULAR_MODE} {DUMMY_OID} 0\t{rel}")
    }

    /// `Tracked` を歯の失敗文へ出せる短い名前にする（`Debug` を実装せずに済ませる）。
    fn describe(tracked: &Tracked) -> String {
        match tracked {
            Tracked::Listed(listed) => format!("Listed({} 件)", listed.len()),
            Tracked::NotRepoRoot => "NotRepoRoot".to_owned(),
            Tracked::Unmeasurable(reason) => format!("Unmeasurable({reason})"),
        }
    }

    /// repo の外に一意な作業 dir の path を作る（同 process 内の衝突を避ける）。
    fn tmp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.subsec_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "xtask-paths-clean-{}-{nanos}",
            std::process::id()
        ))
    }

    /// fixture の dir を git repo にする（rc だけを見る・外の設定も HOME も読まない）。
    fn git_init(dir: &Path) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["init", "-q"])
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    /// `git ls-files -s` の 1 件でも形が読めなければ **母集団ごと測れなかった**へ倒す。
    ///
    /// 読めた分だけで走査すると、母集団の欠けが「違反 0 件」として静かに合格する
    /// （測れなかったを異常なしへ落とさない極性）。件数を名指すのは、何件落ちたかを
    /// 事後に数えられるようにするためである。
    #[test]
    fn paths_clean_is_unmeasurable_when_ls_files_entry_is_malformed() {
        let sane = vec![entry("README.md"), entry("docs/design/pipeline.md")];
        let listed = listed_from(&sane);
        assert!(
            matches!(&listed, Tracked::Listed(files) if files.len() == 2),
            "形の揃った 2 件は母集団になるはず: {}",
            describe(&listed)
        );

        // 3 件目だけ TAB を欠く（`parse_ls_entry` が `None` を返す形）。
        let mut mixed = sane.clone();
        mixed.push(format!("{REGULAR_MODE} {DUMMY_OID} 0 docs/no-tab.md"));
        let mixed = listed_from(&mixed);
        match &mixed {
            Tracked::Unmeasurable(reason) => assert!(
                reason.contains("3 件中 2 件"),
                "読めなかった件数を名指すはず: {reason}"
            ),
            other => panic!(
                "1 件でも読めなければ Unmeasurable のはず: {}",
                describe(other)
            ),
        }
    }

    /// index に在るのに作業木で読めない tracked file は **違反 1 件**として数える。
    ///
    /// 無言で skip すると、消えた file・permission の無い file の中身が測られないまま
    /// 合格する。走査本数は読めた分だけを数える（読めなかった 1 本を数に混ぜない）。
    #[test]
    fn paths_clean_counts_unreadable_tracked_file_as_violation() {
        let dir = tmp_dir();
        fs::create_dir_all(&dir).expect("fixture の dir を作れる");
        fs::write(dir.join("readable.md"), b"nothing private here\n")
            .expect("fixture の file を書ける");

        let listed = vec![
            TrackedFile {
                rel: "readable.md".to_owned(),
                mode: REGULAR_MODE.to_owned(),
                oid: DUMMY_OID.to_owned(),
            },
            TrackedFile {
                rel: "vanished.md".to_owned(),
                mode: REGULAR_MODE.to_owned(),
                oid: DUMMY_OID.to_owned(),
            },
        ];
        let measured = scan_private_paths(&dir, &listed);
        fs::remove_dir_all(&dir).ok();

        assert_eq!(
            measured.fact, "paths-clean=1",
            "走査本数は読めた 1 本だけのはず"
        );
        assert_eq!(
            measured.violations.len(),
            1,
            "読めない 1 本が違反のはず: {:?}",
            measured.violations
        );
        let head = measured
            .violations
            .first()
            .map(String::as_str)
            .unwrap_or_default();
        assert!(
            head.contains("vanished.md"),
            "読めない tracked file の path を名指すはず: {head}"
        );
        // 負例。読めた file まで違反にする実装（`Ok` 枝でも push する形）はここで落ちる。
        assert!(
            !head.contains("readable.md"),
            "読めた file を違反にしないはず: {head}"
        );
    }

    /// 母集団を **測れなかった**周は違反として出す（`n/a` で無違反へ落とさない）。
    ///
    /// 「測れなかった」が「異常なし」に化けると paths-clean は恒久 fail-open になる。
    /// 上の歯が固定するのは Unmeasurable を**作る**側で、ここは作った Unmeasurable を
    /// **違反へ倒す**側＝門そのものである。
    #[test]
    fn paths_clean_reports_unmeasurable_population_as_violation() {
        let dir = tmp_dir();
        fs::create_dir_all(&dir).expect("fixture の dir を作れる");
        // git repo の外を root にすると `git rev-parse --show-toplevel` が rc≠0 になり、
        // 母集団は NotRepoRoot ではなく Unmeasurable へ落ちる。
        let layout = Layout {
            root: dir.clone(),
            core_dir: dir.clone(),
            member_dirs: Vec::new(),
            name: "probe".to_owned(),
        };
        let measured = measure(&layout);
        fs::remove_dir_all(&dir).ok();

        assert_eq!(
            measured.fact, "paths-clean=?",
            "測れなかった周は走査本数を出さないはず"
        );
        assert!(
            !measured.violations.is_empty(),
            "測れなかったは違反として出すはず（n/a で無違反にしない）"
        );
        let head = measured
            .violations
            .first()
            .map(String::as_str)
            .unwrap_or_default();
        assert!(
            head.starts_with("paths-clean: "),
            "違反 1 行は paths-clean の tag を名乗るはず: {head}"
        );
    }

    /// `git ls-files -s` の meta は **mode / oid / stage の 3 列が要る**。
    ///
    /// 1 列でも欠けた件を読めたことにすると、形の違う index 出力が母集団へ紛れ込んだまま
    /// [`listed_from`] の件数一致（読めた == 全件）が通る。母集団の欠けを静かな 0 件へ
    /// 落とさない極性は、まず 1 件の parse がここで `None` を返すことに立っている。
    #[test]
    fn paths_clean_ls_entry_needs_mode_oid_and_stage() {
        let rel = "docs/design/pipeline.md";
        let Some(parsed) = parse_ls_entry(&entry(rel)) else {
            panic!("mode / oid / stage の揃った 1 件は読めるはず");
        };
        assert_eq!(parsed.rel, rel, "path は入力どおりのはず");
        assert_eq!(parsed.mode, REGULAR_MODE, "mode は入力どおりのはず");
        assert_eq!(parsed.oid, DUMMY_OID, "oid は入力どおりのはず");

        // meta を末尾から 1 列ずつ削る（stage 欠け → oid 欠け → mode 欠け）。TAB は残すので、
        // 落ちる理由は「列が足りない」だけに絞られる。
        let short = [
            format!("{REGULAR_MODE} {DUMMY_OID}"),
            REGULAR_MODE.to_owned(),
            String::new(),
        ];
        for meta in short {
            let part = format!("{meta}\t{rel}");
            assert!(
                parse_ls_entry(&part).is_none(),
                "meta が「{meta}」の 1 件は読めないはず"
            );
        }
    }

    /// private path 形の **2 本目**（home dir の短縮展開記号 + 区切り）も違反として数える。
    ///
    /// needle 集合のどれか 1 形でも数えなければ、その形で書かれた path は恒久的に素通りする。
    /// needle の字面は [`PRIVATE_PATH_MARKS`] から取る——歯の source へ書くと paths-clean が
    /// 本 file 自身を撃つ（この歯を足す便が自分で赤くなる）。
    #[test]
    fn paths_clean_flags_tilde_slash_mark() {
        let Some(mark) = PRIVATE_PATH_MARKS.get(1) else {
            panic!("private path 形は 2 形あるはず");
        };
        let dir = tmp_dir();
        fs::create_dir_all(&dir).expect("fixture の dir を作れる");
        fs::write(dir.join("clean.md"), b"nothing private here\nsecond line\n")
            .expect("needle の無い fixture を書ける");
        fs::write(
            dir.join("dirty.md"),
            format!("first line is clean\nstate = {mark}state/live\nthird line\n"),
        )
        .expect("needle の在る fixture を書ける");

        let listed = match listed_from(&[entry("clean.md"), entry("dirty.md")]) {
            Tracked::Listed(files) => files,
            other => panic!("形の揃った 2 件は母集団になるはず: {}", describe(&other)),
        };
        let measured = scan_private_paths(&dir, &listed);
        fs::remove_dir_all(&dir).ok();

        assert_eq!(measured.fact, "paths-clean=2", "2 本とも走査したはず");
        assert_eq!(
            measured.violations.len(),
            1,
            "2 本目の needle を 1 件だけ数えるはず: {:?}",
            measured.violations
        );
        let head = measured
            .violations
            .first()
            .map(String::as_str)
            .unwrap_or_default();
        assert!(
            head.contains("dirty.md:2"),
            "違反の path と行番号を名指すはず: {head}"
        );
        // 負例。needle の無い file まで違反にする実装はここで落ちる。
        assert!(
            !head.contains("clean.md"),
            "needle の無い file を違反にしないはず: {head}"
        );
    }

    /// tracked file が **0 件**の周は「違反 0 件」ではなく **測れなかった**である。
    ///
    /// index の空を数え上げだけで通すと、母集団を 1 本も見ていない周が `paths-clean=0` の
    /// 緑で landing する。0 件は正常な状態ではない（測る木は必ず tracked file を持つ）ので、
    /// 門は 0 件を違反へ倒す。
    #[test]
    fn paths_clean_fails_on_zero_tracked_files() {
        let dir = tmp_dir();
        fs::create_dir_all(&dir).expect("fixture の dir を作れる");
        assert!(git_init(&dir), "fixture を git repo にできるはず");
        // `git add` を撃たないので index は空のまま＝母集団が 0 件になる。
        let layout = Layout {
            root: dir.clone(),
            core_dir: dir.clone(),
            member_dirs: Vec::new(),
            name: "probe".to_owned(),
        };
        let measured = measure(&layout);
        fs::remove_dir_all(&dir).ok();

        assert_eq!(
            measured.fact, "paths-clean=?",
            "0 件の周は走査本数を出さないはず"
        );
        assert!(
            !measured.violations.is_empty(),
            "0 件は違反として出すはず（走査 0 本の緑にしない）"
        );
        let head = measured
            .violations
            .first()
            .map(String::as_str)
            .unwrap_or_default();
        assert!(
            head.starts_with("paths-clean: "),
            "違反 1 行は paths-clean の tag を名乗るはず: {head}"
        );
        assert!(head.contains("0 件"), "0 件であることを名指すはず: {head}");
    }

    /// repo root **でない** dir を root にした周は `n/a` を名乗り、違反を出さない。
    ///
    /// flip-check の base tree は repo の内側の別 dir を root にして撃つので、この枝を
    /// 違反へ倒すと base 健全性の前段が恒久 RED になる。`n/a` は「測らなかった」であって
    /// 「測れなかった」ではない——後者は上の歯が違反へ倒す側である。
    #[test]
    fn paths_clean_is_na_outside_repo_root() {
        let dir = tmp_dir();
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).expect("fixture の dir を作れる");
        assert!(git_init(&dir), "fixture を git repo にできるはず");
        // root は repo の内側だが toplevel ではない（toplevel は 1 つ上の dir）。
        let layout = Layout {
            root: sub.clone(),
            core_dir: sub.clone(),
            member_dirs: Vec::new(),
            name: "probe".to_owned(),
        };
        let measured = measure(&layout);
        fs::remove_dir_all(&dir).ok();

        assert_eq!(
            measured.fact, "paths-clean=n/a(not-a-repo-root)",
            "toplevel でない root は n/a を名乗るはず"
        );
        assert!(
            measured.violations.is_empty(),
            "n/a の周は違反を出さないはず: {:?}",
            measured.violations
        );
    }

    /// [`find_all`] は needle と haystack が **同じ長さ**でも当てる。
    ///
    /// 早期 return の境界を `<=` にすると、本文が needle 1 個ちょうどの file（改行の無い
    /// 1 行 file）が丸ごと素通りする。短い haystack・空 needle・重なりの無い 2 箇所も
    /// 同じ 1 本で並べる——境界を別々の歯に散らすと、片側だけ直した実装が通る。
    #[test]
    fn paths_clean_find_all_matches_needle_equal_to_haystack() {
        let empty: Vec<usize> = Vec::new();
        assert_eq!(
            find_all(b"ab", b"ab"),
            vec![0],
            "needle と haystack が同じ長さでも当たるはず"
        );
        assert_eq!(
            find_all(b"a", b"ab"),
            empty,
            "haystack が needle より短ければ当たらないはず"
        );
        assert_eq!(find_all(b"ab", b""), empty, "空の needle は当たらないはず");
        assert_eq!(
            find_all(b"abab", b"ab"),
            vec![0, 2],
            "重なりの無い 2 箇所を昇順で返すはず"
        );
    }
}
