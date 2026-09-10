//! claude-md-constitution（憲法の規範文を `CLAUDE.md` の生成区間へ注入する）。
//!
//! 憲法は全 role の context に**機械で**載っていなければならない。注入の経路は
//! `CLAUDE.md` **1 本**である——`claude -p` は worktree でも project の `CLAUDE.md` を
//! 読むので、hook や prompt template へ重ねて入れると同じ規範文が 2 面から来る。
//!
//! 抽出は **HTML の属性**で行い prose の字面 grep では行わない（憲法 C1.2「人向けは
//! 生成・手書き 0 行」）。母集団は `data-audience="machine"` の面に在る `p` / `li` / `td`
//! のうち MUST か SHALL を含むもので、**件数は抽出器の定義で決まる**ので契約にも本 file
//! にも焼かない（判定行が実測値を出す）。
//!
//! **改訂の削除側は本文ごと落とす**。憲法の改訂形は `<del class="delta">旧</del>
//! `<ins class="delta">新</ins>` の対（N4.2）なので、tag を剥がすだけだと**超過された
//! 旧規範文が CLAUDE.md へ載る**（実測: C8.2 / C8.3 / A4 / A4.2 の 4 段落が該当）。
//!
//! parse 不能は loud（rc≠0）である——「読めなかった」を「規範文 0 本」に化けさせない。

use crate::check::{failed, Layout, Measured};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// 抽出元（repo root からの相対）。
const SOURCE_REL: &str = "design-intent/spec/constitution.html";

/// 書き先（repo root からの相対）。
const TARGET_REL: &str = "CLAUDE.md";

/// 生成区間の始まりの印。
pub const BEGIN: &str = "<!-- constitution:begin -->";

/// 生成区間の終わりの印。
pub const END: &str = "<!-- constitution:end -->";

/// 判定行に出す tag。
const TAG: &str = "claude-md-constitution";

/// 母集団に採る要素名。
const CAPTURED: &[&str] = &["p", "li", "td"];

/// 規範文の印（RFC 2119 / RFC 8174）。どちらかを含む段落だけを採る。
const NORMATIVE: &[&str] = &["MUST", "SHALL"];

/// 機械層の宣言（`data-audience` の値）。
const MACHINE: &str = "machine";

/// audience を宣言する属性の名。
const AUDIENCE_ATTR: &str = "data-audience";

/// 条 id を運ぶ span の class。
const ID_CLASS: &str = "ears-id";

/// 本文ごと落とす要素（改訂の削除側）。
const DROPPED: &str = "del";

/// 中身を tag として読まない要素（`<` `>` を含む script 本文で tag を誤読しない）。
const RAW: &[&str] = &["script", "style"];

/// 閉じ tag を持たない要素。stack へ積むと以降の入れ子が 1 段ずれる。
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// 実体参照の読み替え（本 file に現れる形だけ）。未知の参照は**そのまま残す**。
const ENTITIES: &[(&str, &str)] = &[
    ("&lt;", "<"),
    ("&gt;", ">"),
    ("&quot;", "\""),
    ("&#39;", "'"),
    ("&nbsp;", " "),
    ("&middot;", "·"),
    // `&amp;` は最後に読む（先に読むと `&amp;lt;` が `<` に化ける）。
    ("&amp;", "&"),
];

/// 読み取った tag 1 つ。
struct Tag<'a> {
    /// 要素名（小文字化しない＝本 file は小文字で書かれている）。
    name: &'a str,
    /// `<name` と `>` の間（属性の並び）。
    attrs: &'a str,
    /// 閉じ tag か。
    closing: bool,
    /// `/>` で閉じているか。
    self_closing: bool,
}

/// 開いている要素 1 つ。
struct Frame<'a> {
    /// 要素名。
    name: &'a str,
    /// この要素が宣言する audience（宣言が無ければ `None`＝親を継ぐ）。
    audience: Option<&'a str>,
    /// この要素の `id` 属性（条 id の代替に使う）。
    id: Option<&'a str>,
}

/// 採取中の段落 1 つ。
struct Capture {
    /// 採取を始めた要素の stack 深さ（この深さまで戻ったら閉じる）。
    depth: usize,
    /// 条 id（`ears-id` span から採る）。
    id: Option<String>,
    /// 本文。
    body: String,
    /// 条 id を採っている区間の深さ。
    naming: Option<usize>,
}

impl Capture {
    /// いま text をどこへ足すか。
    fn take(&mut self, text: &str) {
        if self.naming.is_some() {
            self.id.get_or_insert_with(String::new).push_str(text);
            return;
        }
        self.body.push_str(text);
    }
}

/// 走査の途中状態。
///
/// **削除側（`<del>`）の深さは採取の外側でも持つ**。`Capture` の中だけで覚えると、条を
/// block ごと打ち消す改訂（`<del><p>…</p></del>` / `<ul><del><li>…</li></del></ul>`）で
/// `<p>` に入った時点の削除側が見えず、**超過された条がそのまま抽出される**（実測
/// 2026-09-10 の probe: 2 段落が生き残った）。現行の憲法は inline の `<del>` しか使って
/// いないが、generate と measure は同じ抽出器を共有するので**この穴では drift が立たない**
/// ——CI が緑のまま死んだ条が全 role の context に載る形になる。
struct Walk<'a> {
    /// 開いている要素。
    stack: Vec<Frame<'a>>,
    /// 採取中の段落。
    cap: Option<Capture>,
    /// 本文ごと落としている `<del>` 区間の深さ。
    dropped: Option<usize>,
    /// 組み上がった段落。
    out: Vec<String>,
}

/// 属性の並びから 1 つ引く。
///
/// **名前の前は境界でなければならない**——`id="…"` で素朴に探すと
/// `data-delta-id="…"` に当たる（実測: 憲法の改訂 marker が全部それである）。
fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let mut from = 0_usize;
    loop {
        let at = attrs.get(from..)?.find(&needle)?.saturating_add(from);
        let start = at.saturating_add(needle.len());
        let end = attrs.get(start..)?.find('"')?.saturating_add(start);
        let bounded = at == 0
            || attrs
                .as_bytes()
                .get(at.saturating_sub(1))
                .is_some_and(u8::is_ascii_whitespace);
        if bounded {
            return attrs.get(start..end);
        }
        from = end.saturating_add(1);
    }
}

/// 実体参照を読み替え、空白の連なりを 1 つに畳んで trim する（1 段落 1 行にする）。
fn flatten(raw: &str) -> String {
    let mut text = raw.to_owned();
    for (from, to) in ENTITIES {
        text = text.replace(from, to);
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `<` から始まる 1 tag を読む。返すのは tag と**その後ろ**である。
fn read_tag(rest: &str) -> Result<(Tag<'_>, &str), String> {
    let inner = rest.get(1..).unwrap_or_default();
    let end = inner
        .find('>')
        .ok_or_else(|| format!("閉じない tag が在る: {}", head(rest)))?;
    let raw = inner.get(..end).unwrap_or_default();
    let after = inner.get(end.saturating_add(1)..).unwrap_or_default();
    let closing = raw.starts_with('/');
    let body = raw.trim_start_matches('/');
    let self_closing = body.ends_with('/');
    let body = body.trim_end_matches('/');
    let cut = body
        .find(|ch: char| ch.is_ascii_whitespace())
        .unwrap_or(body.len());
    let name = body.get(..cut).unwrap_or_default();
    if name.is_empty() {
        return Err(format!("名前の無い tag が在る: {}", head(rest)));
    }
    let attrs = body.get(cut..).unwrap_or_default();
    Ok((
        Tag {
            name,
            attrs,
            closing,
            self_closing,
        },
        after,
    ))
}

/// 診断に載せる先頭の一部（長い HTML を丸ごと載せない）。
fn head(rest: &str) -> String {
    rest.chars().take(40).collect()
}

/// comment / doctype を読み飛ばす。読み飛ばしたら**その後ろ**を返す。
fn skip_ignorable(rest: &str) -> Result<Option<&str>, String> {
    for (open, close) in [("<!--", "-->"), ("<!", ">")] {
        if !rest.starts_with(open) {
            continue;
        }
        let from = rest.get(open.len()..).unwrap_or_default();
        let end = from
            .find(close)
            .ok_or_else(|| format!("閉じない {open} が在る: {}", head(rest)))?;
        return Ok(from.get(end.saturating_add(close.len())..));
    }
    Ok(None)
}

/// `script` / `style` の中身を tag として読まずに飛ばす。
fn skip_raw<'a>(rest: &'a str, name: &str) -> Result<&'a str, String> {
    let close = format!("</{name}>");
    let end = rest
        .find(&close)
        .ok_or_else(|| format!("閉じない {name} が在る"))?;
    Ok(rest.get(end.saturating_add(close.len())..).unwrap_or_default())
}

/// stack のうち**いちばん内側の宣言**が machine か。
fn in_machine(stack: &[Frame<'_>]) -> bool {
    stack
        .iter()
        .rev()
        .find_map(|frame| frame.audience)
        .is_some_and(|found| found == MACHINE)
}

impl<'a> Walk<'a> {
    /// tag と tag の間の生の text を積む。
    fn text(&mut self, raw: &str) {
        if self.dropped.is_some() {
            return;
        }
        if let Some(found) = self.cap.as_mut() {
            found.take(raw);
        }
    }

    /// 開き tag を 1 つ処理する。
    fn open(&mut self, tag: &Tag<'a>) {
        let audience = attr(tag.attrs, AUDIENCE_ATTR);
        let id = attr(tag.attrs, "id");
        if !tag.self_closing && !VOID.contains(&tag.name) {
            self.stack.push(Frame {
                name: tag.name,
                audience,
                id,
            });
        }
        let depth = self.stack.len();
        if tag.name == DROPPED && self.dropped.is_none() {
            self.dropped = Some(depth);
            return;
        }
        // **削除側の抑止は [`Walk::text`] 1 本に寄せる**。ここでも採取開始を止めると、
        // 同じことを 2 箇所で守る形になり、どちらか片方を外す変異が歯に当たらない
        // （＝機構が在るのに測れない）。削除側で始まった採取は本文が 1 byte も入らず、
        // 規範語を含まないので [`finish`] が捨てる。
        match self.cap.as_mut() {
            // 採取中: 条 id の区間だけを覚える（他の tag は剥がすだけ）。
            Some(found) => {
                if attr(tag.attrs, "class") == Some(ID_CLASS) && found.naming.is_none() {
                    found.naming = Some(depth);
                }
            }
            // 採取していない: 母集団の要素で machine 面なら採取を始める。
            None => {
                if CAPTURED.contains(&tag.name) && in_machine(&self.stack) {
                    self.cap = Some(Capture {
                        depth,
                        id: None,
                        body: String::new(),
                        naming: None,
                    });
                }
            }
        }
    }
}

/// 採取が終わった段落を 1 本に組む。規範文でなければ捨てる（`None`）。
fn finish(cap: Capture, id_fallback: Option<&str>) -> Result<Option<String>, String> {
    let body = flatten(&cap.body);
    if !NORMATIVE.iter().any(|word| body.contains(word)) {
        return Ok(None);
    }
    let named = cap.id.as_deref().map(flatten).filter(|found| !found.is_empty());
    let id = named
        .or_else(|| id_fallback.map(str::to_owned))
        .ok_or_else(|| format!("条 id を解決できない規範文が在る: {body}"))?;
    // 条 id は**先頭に付け直す**。span の中身は本文から外してあるので、残った
    // 区切りの `:` を落としてから 1 本に組む（`C1: C1: …` にしない）。
    let rest = body.trim_start_matches(':').trim();
    Ok(Some(format!("{id}: {rest}")))
}

/// 閉じ tag を 1 つ処理する。
///
/// **名前の合う frame まで戻す**（`<li>` を閉じずに `</ul>` で閉じる形が HTML には
/// 在る）。stack のどこにも無い閉じ tag は無視する——captured 要素の外の話であり、
/// ここで落とすと憲法の改訂 1 つで CI が読めない理由で赤くなる。
impl Walk<'_> {
    fn close(&mut self, name: &str) -> Result<(), String> {
        let Some(at) = self.stack.iter().rposition(|frame| frame.name == name) else {
            return Ok(());
        };
        let fallback = self
            .stack
            .iter()
            .take(at.saturating_add(1))
            .rev()
            .find_map(|frame| frame.id)
            .map(str::to_owned);
        self.stack.truncate(at);
        let depth = self.stack.len().saturating_add(1);
        if self.dropped.is_some_and(|from| from >= depth) {
            self.dropped = None;
        }
        let Some(found) = self.cap.as_mut() else {
            return Ok(());
        };
        if found.naming.is_some_and(|from| from >= depth) {
            found.naming = None;
        }
        if found.depth < depth {
            return Ok(());
        }
        let taken = self.cap.take().ok_or("採取中の段落が消えた")?;
        if let Some(line) = finish(taken, fallback.as_deref())? {
            self.out.push(line);
        }
        Ok(())
    }
}

/// 憲法 HTML から規範文の段落を抽出する。
pub fn paragraphs(html: &str) -> Result<Vec<String>, String> {
    let mut walk = Walk {
        stack: Vec::new(),
        cap: None,
        dropped: None,
        out: Vec::new(),
    };
    let mut rest = html;
    while let Some(at) = rest.find('<') {
        let (text, tail) = rest.split_at(at);
        // **生の text のまま積む**。chunk ごとに畳んで区切りを足すと、tag の直後の
        // 句読点が離れる（実測: `…in a decision , scribe2 SHALL …`）。空白の畳みは
        // 段落を組む [`finish`] で 1 度だけ行う。
        walk.text(text);
        rest = tail;
        if let Some(after) = skip_ignorable(rest)? {
            rest = after;
            continue;
        }
        let (tag, after) = read_tag(rest)?;
        rest = after;
        if tag.closing {
            walk.close(tag.name)?;
            continue;
        }
        walk.open(&tag);
        if RAW.contains(&tag.name) && !tag.self_closing {
            rest = skip_raw(rest, tag.name)?;
            walk.close(tag.name)?;
        }
    }
    if walk.cap.is_some() {
        return Err("閉じないまま file が終わった段落が在る".to_owned());
    }
    Ok(walk.out)
}

/// 段落の列を生成区間の本文へ組む（1 段落 1 行・空行区切り）。
pub fn body_of(lines: &[String]) -> String {
    lines.join("\n\n")
}

/// `CLAUDE.md` の生成区間の中身を返す（marker の間・両端の marker は含まない）。
pub fn region_of(text: &str) -> Result<&str, String> {
    for (marker, count) in [(BEGIN, text.matches(BEGIN).count()), (END, text.matches(END).count())]
    {
        if count != 1 {
            return Err(format!("{TARGET_REL} の {marker} が 1 本でない（{count} 本）"));
        }
    }
    let from = text.find(BEGIN).ok_or("開始 marker が無い")?.saturating_add(BEGIN.len());
    let to = text.find(END).ok_or("終了 marker が無い")?;
    if to < from {
        return Err(format!("{TARGET_REL} の marker が逆順である"));
    }
    text.get(from..to).ok_or_else(|| "区間を切り出せない".to_owned())
}

/// 生成区間だけを差し替えた `CLAUDE.md` の全文を組む。**区間外は 1 byte も触らない**。
pub fn splice(text: &str, body: &str) -> Result<String, String> {
    let region = region_of(text)?;
    let from = text.find(BEGIN).ok_or("開始 marker が無い")?.saturating_add(BEGIN.len());
    let to = from.saturating_add(region.len());
    let before = text.get(..from).ok_or("marker の前を切り出せない")?;
    let after = text.get(to..).ok_or("marker の後ろを切り出せない")?;
    Ok(format!("{before}\n{body}\n{after}"))
}

/// file を読む（読めない理由を逐語で載せる）。
fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("{} を読めない: {err}", path.display()))
}

/// 抽出元と書き先の path。
fn paths(root: &Path) -> (PathBuf, PathBuf) {
    (root.join(SOURCE_REL), root.join(TARGET_REL))
}

/// 生成区間の本文を組む（抽出 → 1 本の text）。
fn rendered(root: &Path) -> Result<(String, usize), String> {
    let (source, _) = paths(root);
    let lines = paragraphs(&read(&source)?)?;
    if lines.is_empty() {
        return Err(format!("{SOURCE_REL} から規範文を 1 本も抽出できない"));
    }
    let count = lines.len();
    Ok((body_of(&lines), count))
}

/// `gen-claude-md` subcommand の本体。`CLAUDE.md` の生成区間を書き直す。
pub fn generate(root: &Path) -> Result<String, String> {
    let (_, target) = paths(root);
    let (body, count) = rendered(root)?;
    let text = read(&target)?;
    let next = splice(&text, &body)?;
    fs::write(&target, &next).map_err(|err| format!("{} を書けない: {err}", target.display()))?;
    Ok(format!("{TAG}: ok paragraphs={count} bytes={}", body.len()))
}

/// 憲法を持たない workspace か（xtask の歯が組む骨格だけの木がこれである）。
///
/// **`n/a` へ倒すのはここだけ**である。抽出元が **NotFound のときに限り**「測る対象が
/// 無い」と名乗り、読めない / 壊れている / 書き先が無い / 区間が無いはすべて deny へ倒す
/// ——「測れなかった」を「異常なし」に化けさせないためで、極性は paths-clean の
/// `n/a(not-a-repo-root)` と同じ形である。
fn absent(source: &Path) -> Result<bool, String> {
    match fs::metadata(source) {
        Ok(_) => Ok(false),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(true),
        Err(err) => Err(format!("{} を stat できない: {err}", source.display())),
    }
}

/// 抽出元が無い木の扱い。**書き先に生成区間が残っていれば deny** である。
///
/// 憲法を持たない workspace（xtask の歯が組む骨格だけの木）は測る対象が無いので `n/a` を
/// 名乗ってよい。しかし `CLAUDE.md` に生成区間が在るなら、それは「対象が無い」ではなく
/// **正本を失った規範文が全 role の context に残っている**状態＝drift そのものである。
/// paths-clean の `n/a(not-a-repo-root)` は「その木が repo ですらない」という適用外の
/// 宣言で、こちらとは形が違う。★実測 2026-09-10: 憲法を `git mv` で **tracked ごと**動かすと
/// paths-clean も拾わないので、狭めなければ `rc 0` のまま 57 段落が孤児化する。
fn no_source(target: &Path) -> Measured {
    let orphan = read(target).is_ok_and(|text| region_of(&text).is_ok());
    if orphan {
        return failed(
            TAG,
            &format!("{SOURCE_REL} が無いのに {TARGET_REL} に生成区間が在る（正本を失った規範文である）"),
        );
    }
    Measured {
        fact: format!("{TAG}=n/a(no-constitution)"),
        violations: Vec::new(),
    }
}

/// `cargo xtask check` の measure。区間の中身が生成結果と byte 一致しなければ deny。
pub(crate) fn measure(layout: &Layout) -> Measured {
    let (source, target) = paths(&layout.root);
    match absent(&source) {
        Ok(true) => return no_source(&target),
        Ok(false) => {}
        Err(reason) => return failed(TAG, &reason),
    }
    let (body, count) = match rendered(&layout.root) {
        Ok(found) => found,
        Err(reason) => return failed(TAG, &reason),
    };
    let text = match read(&target) {
        Ok(found) => found,
        Err(reason) => return failed(TAG, &reason),
    };
    let region = match region_of(&text) {
        Ok(found) => found,
        Err(reason) => return failed(TAG, &reason),
    };
    if region.trim().is_empty() {
        return failed(TAG, &format!("{TARGET_REL} の生成区間が空である"));
    }
    if region != format!("\n{body}\n") {
        return failed(
            TAG,
            &format!("{TARGET_REL} の生成区間が {SOURCE_REL} と食い違う（cargo xtask gen-claude-md で直す）"),
        );
    }
    Measured {
        fact: format!("{TAG}={count}"),
        violations: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        attr, body_of, generate, measure, paragraphs, region_of, splice, BEGIN, END,
    };
    use crate::check::Layout;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// fixture の憲法 1 本。
    ///
    /// **字面は入力とぶつからない語を選ぶ**（`KEEPSAKE-…` 等）——`SHALL` のような
    /// ありふれた語で照合すると、抽出が 1 文字も働かなくても assert が真になる。
    /// 落ちるべき形を 5 つ並べてある: audience 宣言の無い段落 / human 面の段落 /
    /// machine 面だが規範語の無い段落 / 改訂の削除側 / script の中身。
    const FIXTURE: &str = concat!(
        "<!DOCTYPE html>\n<html><head><meta charset=\"UTF-8\">",
        "<!-- comment with SHALL and OMITTED-MU -->",
        "</head><body>\n",
        "<p>OMITTED-NU scribe2 SHALL be outside every audience declaration.</p>\n",
        "<section id=\"s1\" data-audience=\"human\">\n",
        "  <p>OMITTED-XI scribe2 SHALL sit on the human side.</p>\n",
        "  <details data-audience=\"machine\">\n",
        "    <p class=\"ears\"><span class=\"ears-id\">X1</span>: scribe2 ",
        "<span class=\"ears-shall\">SHALL</span> hold KEEPSAKE-ALPHA in one place.</p>\n",
        "    <p>OMITTED-OMICRON is machine but carries no normative keyword.</p>\n",
        "    <p class=\"ears\"><del class=\"delta\" data-delta-id=\"D-1\">",
        "<span class=\"ears-id\">X2</span>: scribe2 SHALL hold REPEALED-GAMMA.</del>",
        "<ins class=\"delta\" data-delta-id=\"D-1\">",
        "<span class=\"ears-id\">X2</span>: scribe2 SHALL hold LIVING-DELTA.</ins></p>\n",
        "    <p class=\"ears\"><del class=\"delta\" data-delta-id=\"D-2\">",
        "<span class=\"ears-id\">X3</span>: scribe2 SHALL hold VANISHED-EPSILON.</del></p>\n",
        "    <script>if (a &lt; b) { /* OMITTED-PI SHALL not be read */ }</script>\n",
        "  </details>\n</section>\n",
        "<section data-delta-id=\"D-3\" id=\"s2-fallback\" data-audience=\"machine\">\n",
        "  <ul><li>MUST hold ANCHORED-ETA under the fallback id</li>",
        "<li>OMITTED-RHO has no keyword</li></ul>\n",
        "  <table><tr><td>scribe2 SHALL hold CELLED-THETA</td>",
        "<td>OMITTED-SIGMA has no keyword</td></tr></table>\n",
        "</section>\n</body></html>\n",
    );

    /// [`FIXTURE`] から出るべき段落（順序込み）。
    const EXPECTED: &[&str] = &[
        "X1: scribe2 SHALL hold KEEPSAKE-ALPHA in one place.",
        "X2: scribe2 SHALL hold LIVING-DELTA.",
        "s2-fallback: MUST hold ANCHORED-ETA under the fallback id",
        "s2-fallback: scribe2 SHALL hold CELLED-THETA",
    ];

    /// 落ちるべき形の印（1 つでも本文に出たら抽出が広すぎる）。
    const OMITTED: &[&str] = &[
        "OMITTED-MU",
        "OMITTED-NU",
        "OMITTED-XI",
        "OMITTED-OMICRON",
        "OMITTED-PI",
        "OMITTED-RHO",
        "OMITTED-SIGMA",
        "REPEALED-GAMMA",
        "VANISHED-EPSILON",
    ];

    /// repo の外に一意な作業 dir を作る。
    fn tmp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.subsec_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "xtask-claude-md-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("作業 dir を作れる");
        dir
    }

    /// fixture の憲法と `CLAUDE.md` を 1 組置く。
    fn seed(dir: &Path, region: &str) {
        fs::create_dir_all(dir.join("design-intent").join("spec")).expect("fixture の dir を作れる");
        fs::write(
            dir.join("design-intent").join("spec").join("constitution.html"),
            FIXTURE,
        )
        .expect("憲法 fixture を書ける");
        fs::write(dir.join("CLAUDE.md"), claude_md_text(region)).expect("CLAUDE.md を書ける");
    }

    /// marker 区間に `region` を持つ `CLAUDE.md` の全文。
    fn claude_md_text(region: &str) -> String {
        format!("# fixture\n\nBEFORE-KAPPA\n{BEGIN}{region}{END}\nAFTER-LAMBDA\n")
    }

    /// `root` だけを使う [`Layout`]（measure は root しか読まない）。
    fn layout_of(dir: &Path) -> Layout {
        Layout {
            root: dir.to_path_buf(),
            core_dir: dir.join("crates").join("core"),
            member_dirs: Vec::new(),
            name: "fixture".to_owned(),
        }
    }

    /// 母集団は machine 面の p / li / td のうち規範語を含むものだけである。
    #[test]
    fn claude_md_extracts_only_machine_normative_paragraphs() {
        let lines = paragraphs(FIXTURE).expect("fixture を parse できる");
        assert_eq!(lines, EXPECTED, "抽出した段落");
        let body = body_of(&lines);
        for mark in OMITTED {
            assert!(!body.contains(mark), "落ちるべき形が残った: {mark}");
        }
    }

    /// 改訂の削除側は本文ごと落ち、丸ごと削除された条は 1 行も残らない。
    #[test]
    fn claude_md_drops_superseded_delta_and_whole_repeals() {
        let body = body_of(&paragraphs(FIXTURE).expect("fixture を parse できる"));
        assert!(body.contains("X2: scribe2 SHALL hold LIVING-DELTA."), "生きた側: {body}");
        assert!(!body.contains("REPEALED-GAMMA"), "超過された旧文が残った");
        assert!(!body.contains("X3"), "丸ごと削除された条が残った");
    }

    /// 段落は祖先の閉じ tag でも閉じる（`<li>` を閉じない形の HTML）。
    #[test]
    fn claude_md_closes_paragraph_at_ancestor_end_tag() {
        let html = concat!(
            "<section id=\"s3\" data-audience=\"machine\">",
            "<ul><li>MUST hold UNCLOSED-IOTA</ul></section>",
        );
        let lines = paragraphs(html).expect("閉じない li を parse できる");
        assert_eq!(lines, vec!["s3: MUST hold UNCLOSED-IOTA".to_owned()], "抽出した段落");
    }

    /// 差し替えるのは marker の間だけで、区間外は 1 byte も動かない。
    #[test]
    fn claude_md_splices_only_between_markers() {
        let before = claude_md_text("\nOLD-OMEGA\n");
        let after = splice(&before, "NEW-PSI").expect("差し替えられる");
        assert_eq!(after, claude_md_text("\nNEW-PSI\n"), "差し替え後の全文");
        for (side, text) in [("前", "BEFORE-KAPPA"), ("後ろ", "AFTER-LAMBDA")] {
            assert!(after.contains(text), "区間の{side}が消えた");
        }
        assert!(!after.contains("OLD-OMEGA"), "古い区間が残った");
    }

    /// marker が無い / 2 本ある / 逆順は読めない（rc≠0 へ落とす）。
    #[test]
    fn claude_md_refuses_broken_markers() {
        let cases = [
            ("marker 無し", "# fixture\nBEFORE-KAPPA\n".to_owned()),
            ("開始が 2 本", format!("{BEGIN}\na\n{BEGIN}\nb\n{END}\n")),
            ("逆順", format!("{END}\nbody\n{BEGIN}\n")),
        ];
        for (label, text) in cases {
            assert!(region_of(&text).is_err(), "{label} を読めてしまった");
            assert!(splice(&text, "NEW-PSI").is_err(), "{label} を差し替えてしまった");
        }
    }

    /// 生成結果と一致する区間は通り、判定行に段落数が出る。
    #[test]
    fn claude_md_measure_accepts_generated_region() {
        let dir = tmp_dir("ok");
        seed(&dir, "\n");
        generate(&dir).expect("生成できる");
        let measured = measure(&layout_of(&dir));
        assert!(measured.violations.is_empty(), "違反: {:?}", measured.violations);
        assert_eq!(measured.fact, format!("claude-md-constitution={}", EXPECTED.len()));
        fs::remove_dir_all(&dir).ok();
    }

    /// drift / 空 / 区間不在はいずれも deny（判定行の値も `?` へ倒す）。
    #[test]
    fn claude_md_measure_denies_drift_and_empty_and_missing_region() {
        let dir = tmp_dir("deny");
        seed(&dir, "\n");
        generate(&dir).expect("生成できる");
        let generated = fs::read_to_string(dir.join("CLAUDE.md")).expect("生成物を読める");
        let cases = [
            ("drift", generated.replace("KEEPSAKE-ALPHA", "TAMPERED-TAU")),
            // **byte 一致を測る**。本文は 1 文字も違わず端の空行だけ 1 行増えた形は、
            // 比較を trim へ緩めた実装では素通りする（人が marker の直後で改行する形）。
            (
                "端の空行",
                generated.replace(&format!("{BEGIN}\n"), &format!("{BEGIN}\n\n")),
            ),
            ("空", claude_md_text("\n\n")),
            ("区間不在", "# fixture\nBEFORE-KAPPA\n".to_owned()),
        ];
        for (label, text) in cases {
            fs::write(dir.join("CLAUDE.md"), &text).expect("CLAUDE.md を書ける");
            let measured = measure(&layout_of(&dir));
            assert_eq!(measured.violations.len(), 1, "{label} の違反行");
            assert_eq!(measured.fact, "claude-md-constitution=?", "{label} の判定値");
        }
        fs::remove_file(dir.join("CLAUDE.md")).expect("CLAUDE.md を消せる");
        let measured = measure(&layout_of(&dir));
        assert_eq!(measured.violations.len(), 1, "書き先不在の違反行");
        assert_eq!(measured.fact, "claude-md-constitution=?", "書き先不在の判定値");
        fs::remove_dir_all(&dir).ok();
    }

    /// 憲法を持たない workspace は `n/a` を名乗る（骨格だけの木は測る対象が無い）。
    ///
    /// **`n/a` は「抽出元も生成区間も無い」ときだけ**である。区間だけが残る木は
    /// [`claude_md_measure_denies_orphan_region_without_constitution`] が deny 側で、
    /// stat に失敗する木は [`claude_md_measure_denies_when_source_cannot_be_stated`] が
    /// deny 側で押さえる＝この 3 本の組で「測れなかったを異常なしへ落とさない」を測る。
    #[test]
    fn claude_md_measure_reports_na_without_constitution() {
        let dir = tmp_dir("na");
        for (label, body) in [
            ("CLAUDE.md 不在", None),
            ("marker の無い CLAUDE.md", Some("# fixture\nBEFORE-KAPPA\n")),
        ] {
            match body {
                None => {
                    fs::remove_file(dir.join("CLAUDE.md")).ok();
                }
                Some(text) => fs::write(dir.join("CLAUDE.md"), text).expect("CLAUDE.md を書ける"),
            }
            let measured = measure(&layout_of(&dir));
            assert!(measured.violations.is_empty(), "{label}: {:?}", measured.violations);
            assert_eq!(measured.fact, "claude-md-constitution=n/a(no-constitution)", "{label}");
        }
        fs::remove_dir_all(&dir).ok();
    }

    /// 正本を失った生成区間は `n/a` でなく deny。
    ///
    /// 憲法を **tracked ごと**移した便は paths-clean も拾わない（実測 2026-09-10: `git mv`
    /// で `cargo xtask check` が rc 0 のまま通り、出所を失った規範文が CLAUDE.md に残った）。
    #[test]
    fn claude_md_measure_denies_orphan_region_without_constitution() {
        let dir = tmp_dir("orphan");
        seed(&dir, "\n");
        generate(&dir).expect("生成できる");
        fs::remove_file(dir.join("design-intent").join("spec").join("constitution.html"))
            .expect("憲法を消せる");
        let measured = measure(&layout_of(&dir));
        assert_eq!(measured.violations.len(), 1, "違反行: {:?}", measured.violations);
        assert_eq!(measured.fact, "claude-md-constitution=?");
        fs::remove_dir_all(&dir).ok();
    }

    /// 抽出元を stat できない木も deny（適用外にするのは **NotFound だけ**である）。
    #[test]
    fn claude_md_measure_denies_when_source_cannot_be_stated() {
        let dir = tmp_dir("stat");
        // `design-intent` を通常 file にすると、その下の path の stat は NotFound ではなく
        // ENOTDIR で失敗する＝「無い」と「読みに行けない」を弁別できる。
        fs::write(dir.join("design-intent"), "not a directory").expect("塞ぎを書ける");
        // **生成区間を置かない**。区間が在ると孤児の枝でも deny になり、「stat に失敗した
        // から deny した」のか「区間が孤児だから deny した」のかを弁別できない
        // （実測 2026-09-10: 区間を置いた版では `NotFound` の絞りを外す変異が生き残った）。
        fs::write(dir.join("CLAUDE.md"), "# fixture\nBEFORE-KAPPA\n").expect("CLAUDE.md を書ける");
        let measured = measure(&layout_of(&dir));
        assert_eq!(measured.violations.len(), 1, "違反行: {:?}", measured.violations);
        assert_eq!(measured.fact, "claude-md-constitution=?");
        assert!(
            measured.violations.first().is_some_and(|line| line.contains("stat")),
            "stat できなかったと名乗る: {:?}",
            measured.violations
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// `attr()` は名前の**前が境界**であることを要る（`data-delta-id` の罠）。
    ///
    /// 憲法の改訂 marker はすべて `data-delta-id` を持つので、素朴に `id="` を探すと
    /// 条 id の fallback が改訂 id に化ける。
    #[test]
    fn claude_md_attr_requires_a_name_boundary() {
        let attrs = " data-delta-id=\"D-9\" id=\"c1\" data-audience=\"machine\"";
        assert_eq!(attr(attrs, "id"), Some("c1"), "境界のある id を採る");
        assert_eq!(attr(" data-delta-id=\"D-9\"", "id"), None, "接尾一致を採らない");
        assert_eq!(attr(attrs, "data-audience"), Some("machine"), "長い名も採れる");
    }

    /// block ごと `<del>` された条は 1 行も残らない（採取の**外側**でも削除側を見る）。
    ///
    /// 現行の憲法は inline の `<del>` しか使っていないが、generate と measure は同じ
    /// 抽出器を共有するのでこの穴では drift が立たない＝CI が緑のまま死んだ条が載る。
    #[test]
    fn claude_md_drops_block_level_del() {
        let html = concat!(
            "<del class=\"delta\" data-delta-id=\"D-9\"><section id=\"s9\" data-audience=\"machine\">",
            "<p>REPEALED-BLOCK MUST not survive</p></section></del>",
            "<section id=\"s8\" data-audience=\"machine\">",
            "<ul><del><li>REPEALED-ITEM MUST not survive</li></del>",
            "<li>MUST hold SURVIVING-CHI</li></ul></section>",
        );
        let lines = paragraphs(html).expect("parse できる");
        assert_eq!(lines, vec!["s8: MUST hold SURVIVING-CHI".to_owned()], "抽出した段落");
    }

    /// `gen-claude-md` は区間だけを書き直し、2 度撃っても同じ全文になる。
    #[test]
    fn claude_md_generate_rewrites_only_the_region() {
        let dir = tmp_dir("gen");
        seed(&dir, "\nSTALE-UPSILON\n");
        let line = generate(&dir).expect("生成できる");
        assert!(line.starts_with("claude-md-constitution: ok paragraphs=4 "), "判定行: {line}");
        let once = fs::read_to_string(dir.join("CLAUDE.md")).expect("生成物を読める");
        assert!(once.contains("BEFORE-KAPPA") && once.contains("AFTER-LAMBDA"), "区間外が消えた");
        assert!(!once.contains("STALE-UPSILON"), "古い区間が残った");
        generate(&dir).expect("2 度目も生成できる");
        let twice = fs::read_to_string(dir.join("CLAUDE.md")).expect("生成物を読める");
        assert_eq!(once, twice, "2 度撃つと全文が動く");
        fs::remove_dir_all(&dir).ok();
    }
}
