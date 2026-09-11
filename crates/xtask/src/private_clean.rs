//! private-clean（tracked file の本文に PUBLIC 面へ出してはならない **形で判る** needle が
//! 残っていないこと・SRS NFR5・`s2-07l.32`）。
//!
//! paths-clean は path 形（home dir の接頭形と短縮展開記号）しか見ない。本 measure は同じ母集団
//! （index）・同じ読み口（[`crate::paths_clean::body_of`]）・同じ免除（[`crate::paths_clean::exempt`]）
//! の上で、**形だけで判る** 2 種を数える: (a) メールアドレス形のうち RFC 2606 の予約 domain で
//! **ない**もの（`example.com` / `example.net` / `example.org` と TLD `test` / `example` /
//! `invalid` / `localhost` は fixture の常套なので許す）・(b) macOS / Windows の home path 形。
//! host 名・口座名・user の逐語は tracked に needle を置けない（自分自身を撃つ）ので機械の外＝
//! review の領分のまま（planner 裁定 2026-09-11）。
//!
//! 極性は paths-clean と同じ: 読めない file・列挙できない index・0 件は違反（「読めなかった」を
//! 「無かった」に化けさせない）。needle の字面は `concat!` で分けて置く（本 file が自分を撃たない）。
//! 依存は足さない（std の byte 走査だけ・憲法 A3）。

use crate::check::{failed, Layout, Measured};
use crate::paths_clean::{body_of, exempt, find_all, line_of, Tracked, TrackedFile};
use std::collections::BTreeMap;
use std::path::Path;

/// 判定行の tag。
const TAG: &str = "private-clean";

/// (b) home path 形の needle 集合（macOS と Windows の home dir 接頭形・字面は下の `concat!`）。
///
/// 字面をこの file に置くと本 measure が自分自身を撃つので `concat!` で分けて置く
/// （[`crate::limits::PRIVATE_PATH_MARKS`] と同じ理由）。
const HOME_PATH_MARKS: &[&str] = &[concat!("/", "Users", "/"), concat!("\\", "Users", "\\")];

/// RFC 2606 が予約する 2nd-level domain（完全一致か、その配下＝`.` を挟む suffix 一致・大文字小文字を
/// 区別しない）。配下（`mail.example.com`）は第三者に割り当てられないので例示として通す。
/// `example.com.evil.net` は suffix でないので落ちる（前方一致の罠を踏まない）。
const RESERVED_DOMAINS: &[&str] = &["example.com", "example.net", "example.org"];

/// RFC 2606 が予約する TLD（末尾 label の完全一致・大文字小文字を区別しない）。
const RESERVED_TLDS: &[&str] = &["test", "example", "invalid", "localhost"];

/// 違反として数える形の名（違反行に載せる）。
#[derive(Clone, Copy)]
enum Form {
    /// (a) 予約 domain でないメールアドレス形。
    Email,
    /// (b) macOS / Windows の home path 形。
    UsersPath,
}

impl Form {
    /// 違反行の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::UsersPath => "users-path",
        }
    }
}

/// tracked file の本文に形で判る private needle が残っていないこと（private-clean）。
pub(crate) fn measure(layout: &Layout) -> Measured {
    match crate::paths_clean::tracked_files(&layout.root) {
        Tracked::Unmeasurable(reason) => failed(TAG, &reason),
        Tracked::NotRepoRoot => Measured {
            fact: format!("{TAG}=n/a(not-a-repo-root)"),
            violations: Vec::new(),
        },
        Tracked::Listed(listed) if listed.is_empty() => failed(TAG, "tracked file が 0 件である"),
        Tracked::Listed(listed) => scan(&layout.root, &listed),
    }
}

/// tracked file を 1 本ずつ走査する（読めない file は違反・走査本数は読めた分）。
fn scan(root: &Path, listed: &[TrackedFile]) -> Measured {
    let mut violations = Vec::new();
    let mut scanned = 0;
    for file in listed {
        let rel = &file.rel;
        match body_of(root, file) {
            Err(reason) => violations.push(format!(
                "{TAG}: {rel} を読めない: {reason}（読めない tracked file は違反である）"
            )),
            Ok(bytes) => {
                scanned += 1;
                violations.extend(
                    violating_lines(rel, &bytes)
                        .into_iter()
                        .map(|(line, form)| format!("{TAG}: {rel}:{line} {}", form.as_str())),
                );
            }
        }
    }
    Measured {
        fact: format!("{TAG}={scanned}"),
        violations,
    }
}

/// 違反として数える `(行番号, 形)` を昇順で返す。免除は paths-clean と同じ 1 本を通す。
fn violating_lines(rel: &str, bytes: &[u8]) -> Vec<(usize, Form)> {
    let mut by_line: BTreeMap<usize, Form> = BTreeMap::new();
    for at in email_offsets(bytes) {
        by_line.entry(line_of(bytes, at)).or_insert(Form::Email);
    }
    for mark in HOME_PATH_MARKS {
        for at in find_all(bytes, mark.as_bytes()) {
            by_line.entry(line_of(bytes, at)).or_insert(Form::UsersPath);
        }
    }
    let kept = exempt(rel, bytes, by_line.keys().copied().collect());
    kept.into_iter()
        .filter_map(|line| by_line.get(&line).map(|form| (line, *form)))
        .collect()
}

/// local part に使える byte（RFC 5322 の dot-atom の実用集合）。
fn is_local_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-')
}

/// domain の label に使える byte。
fn is_label_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}

/// `@` の byte offset のうち、予約 domain でないメールアドレス形の中心になるものを返す。
///
/// 形 = `<local>@<label>(.<label>)+` で **末尾 label が英字 2 文字以上**（`crate@1.2.3` の
/// ような `名前@版` 形を当てない）。予約 domain（[`RESERVED_DOMAINS`] / [`RESERVED_TLDS`]）は
/// 除く——`example.co.jp` は予約ではないので当てる。
fn email_offsets(bytes: &[u8]) -> Vec<usize> {
    bytes
        .iter()
        .enumerate()
        .filter(|(_, byte)| **byte == b'@')
        .filter(|(at, _)| has_local_part(bytes, *at))
        .filter_map(|(at, _)| domain_after(bytes, at).map(|domain| (at, domain)))
        .filter(|(_, domain)| !is_reserved(domain))
        .map(|(at, _)| at)
        .collect()
}

/// `@` の直前に local part が 1 byte 以上在るか。
fn has_local_part(bytes: &[u8], at: usize) -> bool {
    at.checked_sub(1)
        .and_then(|before| bytes.get(before))
        .is_some_and(|byte| is_local_byte(*byte))
}

/// `@` の直後の domain（label 2 つ以上・末尾 label が英字 2 文字以上）を小文字で返す。
///
/// domain の**直後の 1 文字が `:`** なら scp 形の git remote（`user@host:path`）であって email
/// ではないので `None`（構造で除外・allow 行も規則文も持たない・`s2-07l.107`）。
fn domain_after(bytes: &[u8], at: usize) -> Option<String> {
    let tail = bytes.get(at.checked_add(1)?..)?;
    let end = tail
        .iter()
        .position(|byte| !(is_label_byte(*byte) || *byte == b'.'))
        .unwrap_or(tail.len());
    if tail.get(end) == Some(&b':') {
        return None;
    }
    let raw = tail.get(..end)?.to_ascii_lowercase();
    let text = String::from_utf8(raw).ok()?;
    let trimmed = text.trim_end_matches('.');
    let labels: Vec<&str> = trimmed.split('.').collect();
    let last = labels.last()?;
    let well_formed = labels.len() >= 2
        && labels.iter().all(|label| !label.is_empty())
        && last.len() >= 2
        && last.bytes().all(|byte| byte.is_ascii_alphabetic());
    well_formed.then(|| trimmed.to_owned())
}

/// RFC 2606 の予約 domain か（2nd-level の完全一致・その配下・TLD の完全一致）。
fn is_reserved(domain: &str) -> bool {
    RESERVED_DOMAINS
        .iter()
        .any(|reserved| domain == *reserved || domain.ends_with(&format!(".{reserved}")))
        || domain
            .rsplit('.')
            .next()
            .is_some_and(|tld| RESERVED_TLDS.contains(&tld))
}

#[cfg(test)]
mod tests {
    use super::{measure, scan, violating_lines, Form};
    use crate::paths_clean::TrackedFile;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// 同一 process 内での dir 名衝突を避ける連番。
    static SEQ: AtomicU32 = AtomicU32::new(0);
    /// index が通常 file に付ける mode。
    const REGULAR_MODE: &str = "100644";
    /// 本 mod の歯は blob を読まない（symlink を作らない）ので oid は形だけでよい。
    const DUMMY_OID: &str = "0000000000000000000000000000000000000000";

    /// repo の外に一意な tmp dir を作る（`tempfile` は足さない・憲法 A3）。
    fn tmp_dir() -> PathBuf {
        let base = std::env::temp_dir();
        for _ in 0..8 {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|since| since.subsec_nanos())
                .unwrap_or(0);
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = base.join(format!("xtask-private-clean-{}-{nanos}-{seq}", std::process::id()));
            if fs::create_dir(&dir).is_ok() {
                return dir;
            }
        }
        panic!("一意な tmp dir を 8 回で作れない");
    }

    /// 母集団の 1 件（通常 file）。
    fn tracked(rel: &str) -> TrackedFile {
        TrackedFile {
            rel: rel.to_owned(),
            mode: REGULAR_MODE.to_owned(),
            oid: DUMMY_OID.to_owned(),
        }
    }

    /// needle を**実行時に連結**して作る（歯の source に完成形を置くと本 measure が自分を撃つ）。
    fn email(local: &str, domain: &str) -> String {
        format!("{local}@{domain}")
    }

    /// fixture の dir に file を置いて走査し、後始末を assert より前に済ませる。
    fn scan_fixture(files: &[(&str, String)]) -> super::Measured {
        let dir = tmp_dir();
        for (rel, body) in files {
            fs::write(dir.join(rel), body).expect("fixture を書ける");
        }
        let listed: Vec<TrackedFile> = files.iter().map(|(rel, _)| tracked(rel)).collect();
        let measured = scan(Path::new(&dir), &listed);
        fs::remove_dir_all(&dir).ok();
        measured
    }

    /// (a) 予約 domain でないメールアドレス形は違反・RFC 2606 の予約 domain は通る（負例）。
    /// `example.co.jp` は予約ではない（`example` を含むだけでは許さない）。
    #[test]
    fn private_clean_flags_nonreserved_email_and_passes_rfc2606() {
        let dirty = format!(
            "first line\ncontact: {}\nthird\n",
            email("a", "corp-example.co.jp")
        );
        // 予約 SLD の**配下**（RFC 2606 §3 は example.com を第 2 レベルで予約＝配下は割当不能）も通す。
        let reserved = format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            email("user", "example.com"),
            email("ops", "Example.NET"),
            email("x", "foo.invalid"),
            email("y", "bar.test"),
            email("z", "baz.example"),
            email("w", "q.localhost"),
            email("m", "mail.example.com"),
            email("s", "a.b.EXAMPLE.org"),
        );
        // 前方一致の罠・末尾 `.`・予約 TLD を含む別 TLD は**落ちる**（行番号で 1 対 1 に見る）。
        let tricky = format!(
            "{}\n{}\n{}\n{}\n",
            email("e", "example.com.evil.net"),
            email("d", "corp.co."),
            email("j", "b.test.jp"),
            email("k", "myexample.com"),
        );
        let measured = scan_fixture(&[
            ("dirty.md", dirty),
            ("reserved.md", reserved),
            ("tricky.md", tricky),
        ]);

        assert_eq!(measured.fact, "private-clean=3", "3 本とも走査したはず");
        assert_eq!(
            measured.violations,
            vec![
                "private-clean: dirty.md:2 email".to_owned(),
                "private-clean: tricky.md:1 email".to_owned(),
                "private-clean: tricky.md:2 email".to_owned(),
                "private-clean: tricky.md:3 email".to_owned(),
                "private-clean: tricky.md:4 email".to_owned(),
            ],
            "予約でない domain だけが違反・予約 SLD の配下は通るはず"
        );
    }

    /// `名前@版`（`cargo-insta@1.48.0` の形）は末尾 label が英字でないので当てない。
    /// local part の無い `@x.y`（decorator・handle）も当てない。
    #[test]
    fn private_clean_ignores_crate_at_version_and_bare_at() {
        let body = format!(
            "install {}\nuse {}\n{}\n{}\n",
            email("cargo-insta", "1.48.0"),
            email("", "scope.io"),
            email("vscode-jsonrpc", "8.2.0"),
            // 末尾 label が英字 1 文字の形は email 形と見ない（2 文字以上の線）。
            email("a", "b.c"),
        );
        let measured = scan_fixture(&[("tools.md", body)]);
        assert_eq!(measured.fact, "private-clean=1");
        assert!(measured.violations.is_empty(), "版の形は違反でないはず: {:?}", measured.violations);
    }

    /// scp 形の git remote（`user@host:path`）は email ではない: domain の**直後の 1 文字が `:`**
    /// なら email 形と見なさない（構造で除外・allow 行も規則文も持たない・`.32` lens MED-2・裁定 (c)）。
    /// 負例: 同じ file の `:` 無しの非予約 email は引き続き違反（除外が広すぎない）。
    #[test]
    fn private_clean_skips_scp_remote_style_git_urls() {
        let body = format!(
            "clone: {}:owner/repo.git\ndeploy: {}:srv/app\nmail: {}\n",
            email("git", "github.com"),
            email("deploy", "example-corp.jp"),
            email("a", "corp-example.co.jp"),
        );
        let measured = scan_fixture(&[("remotes.md", body)]);
        assert_eq!(measured.fact, "private-clean=1");
        assert_eq!(
            measured.violations,
            vec!["private-clean: remotes.md:3 email".to_owned()],
            "scp 形の 2 行は通り、`:` 無しの email だけが違反のはず"
        );
    }

    /// fail-closed（measure の枝）: 母集団 0 件は違反・index を取れない周（git の外）も違反で、
    /// どちらも fact は `?`（走査本数を出さない・0 件の緑にしない）。paths-clean と同じ極性。
    #[test]
    fn private_clean_measure_fails_closed_on_zero_or_unmeasurable_population() {
        use crate::check::Layout;
        let dir = tmp_dir();
        let layout = |root: &Path| Layout {
            root: root.to_path_buf(),
            core_dir: root.to_path_buf(),
            member_dirs: Vec::new(),
            name: "probe".to_owned(),
        };
        // git の外 = `rev-parse --show-toplevel` が rc≠0 → Unmeasurable。
        let outside = measure(&layout(&dir));
        // git repo だが index が空 = 母集団 0 件。
        let inited = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["init", "-q"])
            .output()
            .is_ok_and(|out| out.status.success());
        let empty = measure(&layout(&dir));
        fs::remove_dir_all(&dir).ok();

        assert!(inited, "fixture を git repo にできるはず");
        for (label, measured) in [("git の外", outside), ("0 件", empty)] {
            assert_eq!(measured.fact, "private-clean=?", "{label}: 走査本数を出さないはず");
            let head = measured.violations.first().map(String::as_str).unwrap_or_default();
            assert!(head.starts_with("private-clean: "), "{label}: 違反として出すはず: {head}");
        }
    }

    /// repo root **でない** dir（repo の内側の sub dir）は `n/a` を名乗り違反を出さない
    /// （flip-check の base 木はこの枝＝違反へ倒すと以後の全 PR の flip-check が止まる・lens-87）。
    #[test]
    fn private_clean_is_na_outside_repo_root() {
        use crate::check::Layout;
        let dir = tmp_dir();
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).expect("sub dir を作れる");
        let inited = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["init", "-q"])
            .output()
            .is_ok_and(|out| out.status.success());
        let measured = measure(&Layout {
            root: sub.clone(),
            core_dir: sub.clone(),
            member_dirs: Vec::new(),
            name: "probe".to_owned(),
        });
        fs::remove_dir_all(&dir).ok();

        assert!(inited, "fixture を git repo にできるはず");
        assert_eq!(measured.fact, "private-clean=n/a(not-a-repo-root)");
        assert!(measured.violations.is_empty(), "n/a の周は違反を出さないはず: {:?}", measured.violations);
    }

    /// 免除は paths-clean と**同じ 1 本**を通す: 免除 file のコメント行だけが免除され、同じ file の
    /// 非コメント行の email は違反として残る（免除を外す変異・全行に広げる変異はここで落ちる）。
    #[test]
    fn private_clean_shares_comment_line_exemption_with_paths_clean() {
        use crate::check::PATHS_CLEAN_SKIP;
        let dir = tmp_dir();
        let rel = Path::new(PATHS_CLEAN_SKIP);
        fs::create_dir_all(dir.join(rel.parent().unwrap_or(Path::new(""))))
            .expect("免除 file の dir を作れる");
        let body = format!(
            "# example: {}\ncontact: {}\n# note: {}\n",
            email("doc", "corp-example.co.jp"),
            email("ops", "corp-example.co.jp"),
            email("x", "corp-example.co.jp"),
        );
        fs::write(dir.join(rel), body).expect("免除 file を書ける");
        let measured = scan(&dir, &[tracked(PATHS_CLEAN_SKIP)]);
        fs::remove_dir_all(&dir).ok();

        assert_eq!(measured.fact, "private-clean=1");
        assert_eq!(
            measured.violations,
            vec![format!("private-clean: {PATHS_CLEAN_SKIP}:2 email")],
            "コメント行だけ免除・非コメント行は違反のはず"
        );
    }

    /// (b) macOS / Windows の home path 形は違反・Linux の home 接頭形だけの file は本 measure では数えない
    /// （paths-clean の領分・二重に数えない）。
    #[test]
    fn private_clean_flags_users_home_path_forms_but_not_home() {
        let mac = format!("path = {}someone/x\n", concat!("/", "Users", "/"));
        let win = format!("path = C:{}someone\\x\n", concat!("\\", "Users", "\\"));
        let linux = format!("path = {}someone/x\n", concat!("/", "home", "/"));
        let measured = scan_fixture(&[("mac.md", mac), ("win.md", win), ("linux.md", linux)]);

        assert_eq!(measured.fact, "private-clean=3");
        assert_eq!(
            measured.violations,
            vec![
                "private-clean: mac.md:1 users-path".to_owned(),
                "private-clean: win.md:1 users-path".to_owned(),
            ],
            "macOS と Windows の形だけが違反のはず"
        );
    }

    /// fail-closed: index に在るのに読めない tracked file は違反 1 件（走査本数は読めた分）。
    #[test]
    fn private_clean_counts_unreadable_tracked_file_as_violation() {
        let dir = tmp_dir();
        fs::write(dir.join("readable.md"), b"nothing private here\n").expect("fixture を書ける");
        let listed = vec![tracked("readable.md"), tracked("vanished.md")];
        let measured = scan(&dir, &listed);
        fs::remove_dir_all(&dir).ok();

        assert_eq!(measured.fact, "private-clean=1", "読めた 1 本だけを数えるはず");
        assert_eq!(measured.violations.len(), 1, "{:?}", measured.violations);
        let head = measured.violations.first().map(String::as_str).unwrap_or_default();
        assert!(head.contains("vanished.md") && head.contains("読めない"), "{head}");
        assert!(!head.contains("readable.md"), "読めた file を違反にしないはず: {head}");
    }

    /// 同じ行に 2 形が在れば 1 行 1 件（先に見つけた形）・複数行は行ごとに数える。
    #[test]
    fn private_clean_reports_one_entry_per_line_in_order() {
        let body = format!(
            "{} and {}x\n\n{}y\n",
            email("p", "corp.co"),
            concat!("/", "Users", "/"),
            concat!("/", "Users", "/")
        );
        let found: Vec<(usize, &str)> = violating_lines("multi.md", body.as_bytes())
            .into_iter()
            .map(|(line, form)| (line, form.as_str()))
            .collect();
        assert_eq!(found, vec![(1, Form::Email.as_str()), (3, Form::UsersPath.as_str())]);
    }
}
