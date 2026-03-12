use lopdf::{Document, Object, ObjectId};
use regex::Regex;
use std::path::PathBuf;

pub fn doc_info(input: PathBuf, password: &Option<String>) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;
    println!("\n--- 文档基本信息 ---");
    println!("  PDF 版本  : {}", doc.version);
    println!("  总页数    : {}", doc.get_pages().len());
    println!("  对象总数  : {}", doc.objects.len());
    println!(
        "  加密      : {}",
        if doc.trailer.get(b"Encrypt").is_ok() {
            "✅ 已加密"
        } else {
            "❌ 未加密"
        }
    );

    if let Ok(info_id) = doc.trailer.get(b"Info").and_then(|i| i.as_reference()) {
        if let Ok(info) = doc.get_object(info_id)?.as_dict() {
            println!("\n  [Info 字典]");
            for (k, v) in info {
                let key = String::from_utf8_lossy(k);
                let val = match v {
                    Object::String(s, _) => decode_pdf_string(s),
                    other => format!("{:?}", other),
                };
                println!("    /{:<16}: {}", key, val);
            }
        }
    } else {
        println!("  Info      : 无");
    }

    if let Ok(root_id) = doc.trailer.get(b"Root").and_then(|r| r.as_reference())
        && let Ok(root) = doc.get_object(root_id)?.as_dict()
    {
        println!("\n  [Catalog 摘要]");
        for key in &[
            b"PageLayout".as_ref(),
            b"PageMode",
            b"Lang",
            b"Metadata",
            b"Outlines",
            b"AcroForm",
            b"Permissions",
        ] {
            if let Ok(v) = root.get(key) {
                println!("    /{:<16}: {:?}", String::from_utf8_lossy(key), v);
            }
        }
    }
    Ok(())
}

pub fn outline(
    input: PathBuf,
    depth: Option<usize>,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;
    let root_id = doc.trailer.get(b"Root")?.as_reference()?;
    let root = doc.get_object(root_id)?.as_dict()?;

    let outline_id = match root.get(b"Outlines").and_then(|o| o.as_reference()) {
        Ok(id) => id,
        Err(_) => {
            println!("该文档没有书签大纲。");
            return Ok(());
        }
    };
    let outline_dict = doc.get_object(outline_id)?.as_dict()?;
    let count = outline_dict
        .get(b"Count")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    println!("--- 书签大纲 (顶层 {} 项) ---", count);

    if let Ok(first_id) = outline_dict.get(b"First").and_then(|f| f.as_reference()) {
        print_outline_node(&doc, first_id, 0, depth)?;
    } else {
        println!("（大纲为空）");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn auto_outline(
    input: PathBuf,
    output: PathBuf,
    title_prefix: String,
    detect_headings: bool,
    hierarchical: bool,
    preview_headings: bool,
    min_font_size: f32,
    max_per_page: usize,
    chapter_pattern: Option<String>,
    section_pattern: Option<String>,
    subsection_pattern: Option<String>,
    fallback_to_pages: bool,
    force: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let mut doc = crate::util::load_document(&input, password)?;
    let root_id = doc.trailer.get(b"Root")?.as_reference()?;

    let has_outline = doc.get_object(root_id)?.as_dict()?.get(b"Outlines").is_ok();
    if has_outline && !force {
        anyhow::bail!("该文档已经有目录；如需覆盖请显式传入 --force");
    }

    let pages = doc.get_pages();
    if pages.is_empty() {
        anyhow::bail!("文档没有可用页面，无法生成目录");
    }

    let custom_patterns = CustomPatterns::compile(
        chapter_pattern.as_deref(),
        section_pattern.as_deref(),
        subsection_pattern.as_deref(),
    )?;

    let mut entries = if detect_headings {
        let toc_entries = detect_toc_outline_entries(&doc, hierarchical, &custom_patterns)?;
        if !toc_entries.is_empty() {
            toc_entries
        } else {
            detect_outline_entries(
                &doc,
                min_font_size,
                max_per_page,
                hierarchical,
                &custom_patterns,
            )?
        }
    } else {
        Vec::new()
    };
    if entries.is_empty() {
        if detect_headings && !fallback_to_pages {
            anyhow::bail!("未检测到可用标题；可调低 --min-font-size 或加 --fallback-to-pages");
        }
        entries = build_page_outline_entries(&pages, &title_prefix);
    }

    if preview_headings {
        if !detect_headings {
            anyhow::bail!("--preview-headings 需要配合 --detect-headings 使用");
        }
        print_heading_preview(&doc, &entries);
        return Ok(());
    }

    let outline_root_id = next_object_id(&mut doc);
    write_outline_tree(&mut doc, outline_root_id, &entries)?;

    let root = doc.get_object_mut(root_id)?.as_dict_mut()?;
    root.set("Outlines", outline_root_id);
    root.set("PageMode", Object::Name(b"UseOutlines".to_vec()));

    crate::util::save_document(&mut doc, output)?;
    println!("✅ 已生成目录，共 {} 个书签。", entries.len());
    Ok(())
}

fn print_outline_node(
    doc: &Document,
    node_id: ObjectId,
    depth: usize,
    max: Option<usize>,
) -> anyhow::Result<()> {
    if let Some(m) = max
        && depth > m
    {
        return Ok(());
    }
    let node = doc.get_object(node_id)?.as_dict()?;
    if let Ok(title) = node.get(b"Title").and_then(|t| t.as_str()) {
        let indent = "  ".repeat(depth);
        let title_str = if title.starts_with(&[0xFE, 0xFF]) {
            let utf16: Vec<u16> = title[2..]
                .chunks(2)
                .map(|c| ((c[0] as u16) << 8) | c.get(1).copied().unwrap_or(0) as u16)
                .collect();
            String::from_utf16_lossy(&utf16)
        } else {
            String::from_utf8_lossy(title).to_string()
        };
        // 尝试获取目标页
        let dest_page = get_dest_page(doc, node);
        if let Some(pg) = dest_page {
            println!("{}◆ {} → 第{}页", indent, title_str, pg);
        } else {
            println!("{}◆ {}", indent, title_str);
        }
    }
    if let Ok(first) = node.get(b"First").and_then(|f| f.as_reference()) {
        print_outline_node(doc, first, depth + 1, max)?;
    }
    if let Ok(next) = node.get(b"Next").and_then(|n| n.as_reference()) {
        print_outline_node(doc, next, depth, max)?;
    }
    Ok(())
}

/// 解码 PDF 字符串：UTF-16 BE BOM (FE FF) 自动转换，否则按 latin-1 / utf-8 呈现
fn decode_pdf_string(s: &[u8]) -> String {
    if s.starts_with(&[0xFE, 0xFF]) {
        let utf16: Vec<u16> = s[2..]
            .chunks(2)
            .map(|c| ((c[0] as u16) << 8) | c.get(1).copied().unwrap_or(0) as u16)
            .collect();
        String::from_utf16_lossy(&utf16)
    } else {
        String::from_utf8_lossy(s).to_string()
    }
}

fn get_dest_page(doc: &Document, node: &lopdf::Dictionary) -> Option<u32> {
    // 方式1: /Dest 直接指定 [page_ref /Fit...]
    if let Ok(dest) = node.get(b"Dest") {
        let page_ref = match dest {
            Object::Array(arr) => arr.first()?.as_reference().ok()?,
            Object::Reference(r) => *r,
            _ => return None,
        };
        return page_from_ref(doc, page_ref);
    }
    // 方式2: /A (Action) 字典，GoTo / GoToR
    if let Ok(action) = node.get(b"A").and_then(|a| a.as_dict()) {
        let s_type = action.get(b"S").ok().and_then(|v| v.as_name().ok())?;
        if s_type == b"GoTo"
            && let Ok(d) = action.get(b"D")
        {
            let page_ref = match d {
                Object::Array(arr) => arr.first()?.as_reference().ok()?,
                Object::Reference(r) => *r,
                _ => return None,
            };
            return page_from_ref(doc, page_ref);
        }
    }
    None
}

fn page_from_ref(doc: &Document, page_ref: ObjectId) -> Option<u32> {
    for (num, &pid) in doc.get_pages().iter() {
        if pid == page_ref {
            return Some(*num);
        }
    }
    None
}

fn next_object_id(doc: &mut Document) -> ObjectId {
    doc.max_id += 1;
    (doc.max_id, 0)
}

struct OutlineEntry {
    title: String,
    page_id: ObjectId,
    level: usize,
}

fn build_page_outline_entries(
    pages: &std::collections::BTreeMap<u32, ObjectId>,
    title_prefix: &str,
) -> Vec<OutlineEntry> {
    let mut page_entries: Vec<(u32, ObjectId)> = pages.iter().map(|(n, id)| (*n, *id)).collect();
    page_entries.sort_by_key(|(n, _)| *n);
    page_entries
        .into_iter()
        .map(|(page_num, page_id)| OutlineEntry {
            title: format!("{} {} 页", title_prefix, page_num),
            page_id,
            level: 1,
        })
        .collect()
}

fn detect_outline_entries(
    doc: &Document,
    min_font_size: f32,
    max_per_page: usize,
    hierarchical: bool,
    custom_patterns: &CustomPatterns,
) -> anyhow::Result<Vec<OutlineEntry>> {
    let mut entries = Vec::new();
    let pages_map = doc.get_pages();
    let mut page_nums: Vec<u32> = pages_map.keys().cloned().collect();
    page_nums.sort_unstable();

    for page_num in page_nums {
        let page_id = *pages_map
            .get(&page_num)
            .ok_or_else(|| anyhow::anyhow!("页码 {} 不存在", page_num))?;
        let runs = crate::cmd::text::extract_page_text_runs(doc, page_num)?;
        let mut lines = merge_text_runs_into_lines(runs);
        lines = merge_multiline_candidates(lines);
        lines.retain(|line| {
            line.font_size >= min_font_size
                && line.text.chars().count() <= 120
                && !line.text.trim().is_empty()
                && !is_header_footer_noise(&line.text, line.y)
                && !is_likely_noise(&line.text)
        });
        promote_thesis_heading_lines(&mut lines);
        lines.sort_by(|a, b| {
            b.font_size
                .partial_cmp(&a.font_size)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| b.text.chars().count().cmp(&a.text.chars().count()))
        });
        lines.truncate(max_per_page.max(1));
        lines.sort_by(|a, b| {
            b.y.partial_cmp(&a.y)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
        });
        for line in lines {
            let title = normalize_outline_title(&line.text);
            entries.push(OutlineEntry {
                level: if hierarchical {
                    detect_heading_level(&title, line.font_size, custom_patterns)
                } else {
                    1
                },
                title,
                page_id,
            });
        }
    }
    Ok(entries)
}

fn detect_toc_outline_entries(
    doc: &Document,
    hierarchical: bool,
    custom_patterns: &CustomPatterns,
) -> anyhow::Result<Vec<OutlineEntry>> {
    let pages_map = doc.get_pages();
    let mut page_nums: Vec<u32> = pages_map.keys().cloned().collect();
    page_nums.sort_unstable();
    let mut entries = Vec::new();
    let total_pages = pages_map.len() as u32;
    let mut in_toc = false;

    for page_num in page_nums {
        let runs = crate::cmd::text::extract_page_text_runs(doc, page_num)?;
        let mut lines = merge_text_runs_into_lines(runs);
        lines.retain(|line| !is_likely_noise(&line.text));
        let is_toc_page = lines.iter().any(|line| is_toc_heading(&line.text));
        let toc_like_count = lines
            .iter()
            .filter(|line| parse_toc_line(&line.text).is_some())
            .count();
        let is_toc_continuation = in_toc && toc_like_count >= 3;
        if !is_toc_page && !is_toc_continuation {
            if in_toc {
                break;
            }
            continue;
        }
        in_toc = true;

        let mut page_entries = 0usize;
        for line in lines {
            if let Some((title, target_page)) = parse_toc_line(&line.text) {
                if target_page < 1 || target_page > total_pages {
                    continue;
                }
                if let Some(&page_id) = pages_map.get(&target_page) {
                    let level = if hierarchical {
                        detect_heading_level(&title, line.font_size, custom_patterns)
                    } else {
                        1
                    };
                    entries.push(OutlineEntry {
                        title,
                        page_id,
                        level,
                    });
                    page_entries += 1;
                }
            }
        }
        if in_toc && page_entries == 0 {
            break;
        }
    }

    Ok(entries)
}

#[derive(Clone)]
struct LineCandidate {
    x: f32,
    y: f32,
    font_size: f32,
    text: String,
}

fn merge_text_runs_into_lines(runs: Vec<crate::cmd::text::TextRun>) -> Vec<LineCandidate> {
    let mut sorted = runs;
    sorted.sort_by(|a, b| {
        b.y.partial_cmp(&a.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut lines: Vec<Vec<crate::cmd::text::TextRun>> = Vec::new();
    for run in sorted {
        if let Some(line) = lines.last_mut() {
            let y_close = (line[0].y - run.y).abs() <= 3.0;
            let size_close = (line[0].font_size - run.font_size).abs() <= 1.0;
            if y_close && size_close {
                line.push(run);
                continue;
            }
        }
        lines.push(vec![run]);
    }

    lines
        .into_iter()
        .map(|mut line| {
            line.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
            let x = line.first().map(|r| r.x).unwrap_or(0.0);
            let y = line.first().map(|r| r.y).unwrap_or(0.0);
            let font_size = line
                .iter()
                .map(|r| r.font_size)
                .fold(0.0f32, |acc, v| acc.max(v));
            let text = line
                .iter()
                .map(|r| r.text.trim())
                .collect::<Vec<_>>()
                .join("");
            LineCandidate {
                x,
                y,
                font_size,
                text,
            }
        })
        .collect()
}

fn merge_multiline_candidates(lines: Vec<LineCandidate>) -> Vec<LineCandidate> {
    if lines.is_empty() {
        return lines;
    }
    let mut sorted = lines;
    sorted.sort_by(|a, b| {
        b.y.partial_cmp(&a.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut merged: Vec<LineCandidate> = Vec::new();
    let mut i = 0usize;
    while i < sorted.len() {
        let cur = &sorted[i];
        if i + 1 < sorted.len() && should_merge_lines(cur, &sorted[i + 1]) {
            let next = &sorted[i + 1];
            merged.push(LineCandidate {
                x: cur.x.min(next.x),
                y: cur.y.max(next.y),
                font_size: cur.font_size.max(next.font_size),
                text: format!("{}{}", cur.text.trim_end(), next.text.trim_start()),
            });
            i += 2;
            continue;
        }
        merged.push(cur.clone());
        i += 1;
    }
    merged
}

fn should_merge_lines(a: &LineCandidate, b: &LineCandidate) -> bool {
    let size_close = (a.font_size - b.font_size).abs() <= 1.5;
    let x_close = (a.x - b.x).abs() <= 90.0
        || (looks_like_thesis_title_line(a.text.trim()) && (a.x - b.x).abs() <= 180.0);
    let y_gap = (a.y - b.y).abs();
    let plausible_gap = (8.0..=42.0).contains(&y_gap);
    let a_len = a.text.chars().count();
    let b_len = b.text.chars().count();
    let continuation = !ends_like_complete_heading(&a.text)
        || b_len <= 24
        || a_len <= 24
        || looks_like_thesis_title_line(a.text.trim());
    size_close && x_close && plausible_gap && continuation
}

fn ends_like_complete_heading(s: &str) -> bool {
    let t = s.trim();
    t.ends_with('。')
        || t.ends_with('！')
        || t.ends_with('？')
        || t.ends_with("摘要")
        || t.ends_with("引言")
        || t.ends_with("致谢")
        || t.ends_with("参考文献")
}

fn promote_thesis_heading_lines(lines: &mut [LineCandidate]) {
    for line in lines {
        let t = line.text.trim();
        if is_thesis_anchor_heading(t) {
            line.font_size = line.font_size.max(24.0);
        }
        if looks_like_thesis_title_line(t) {
            line.font_size = line.font_size.max(22.0);
        }
    }
}

fn is_thesis_anchor_heading(s: &str) -> bool {
    matches_fixed_heading(s)
        || s.contains("毕业论文")
        || s.contains("毕业设计")
        || s.contains("学位论文")
}

fn looks_like_thesis_title_line(s: &str) -> bool {
    s.starts_with("题目")
        || s.starts_with("论文题目")
        || s.starts_with("设计题目")
        || s.starts_with("Title")
}

fn normalize_outline_title(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_toc_heading(s: &str) -> bool {
    let t = normalize_outline_title(s);
    t == "目录" || t.eq_ignore_ascii_case("contents") || t.eq_ignore_ascii_case("table of contents")
}

fn parse_toc_line(s: &str) -> Option<(String, u32)> {
    let compact = normalize_outline_title(s);
    if compact.is_empty() || is_toc_heading(&compact) {
        return None;
    }
    let re = Regex::new(r"^(?P<title>.+?)(?:\.|·|…|\s)+(?P<page>\d{1,4})$").ok()?;
    let caps = re.captures(&compact)?;
    let raw_title = caps.name("title")?.as_str().trim();
    let page: u32 = caps.name("page")?.as_str().parse().ok()?;
    let title = raw_title
        .trim_end_matches(['.', '·', '…'])
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }
    Some((title, page))
}

fn detect_heading_level(title: &str, font_size: f32, custom_patterns: &CustomPatterns) -> usize {
    let t = title.trim();
    if custom_patterns
        .chapter
        .as_ref()
        .is_some_and(|re| re.is_match(t))
    {
        return 1;
    }
    if custom_patterns
        .section
        .as_ref()
        .is_some_and(|re| re.is_match(t))
    {
        return 2;
    }
    if custom_patterns
        .subsection
        .as_ref()
        .is_some_and(|re| re.is_match(t))
    {
        return 3;
    }
    if matches_chapter_like(t) {
        return 1;
    }
    if matches_section_like(t) {
        return 2;
    }
    if matches_subsection_like(t) {
        return 3;
    }
    if font_size >= 22.0 {
        1
    } else if font_size >= 18.0 {
        2
    } else {
        3
    }
}

fn matches_chapter_like(s: &str) -> bool {
    starts_with_any(
        s,
        &["第"],
        &["章", "篇", "部分", "卷", "回", "编", "幕", "课", "讲"],
    ) || starts_with_ascii_prefix(s, &["chapter ", "part ", "unit "])
        || starts_with_cn_list_marker(s)
        || leading_number_segments(s) == 1
        || matches_fixed_heading(s)
}

fn matches_section_like(s: &str) -> bool {
    starts_with_any(s, &["第"], &["节", "条"]) || leading_number_segments(s) == 2
}

fn matches_subsection_like(s: &str) -> bool {
    leading_number_segments(s) >= 3 || starts_with_ascii_prefix(s, &["section ", "sec. "])
}

fn starts_with_any(s: &str, prefixes: &[&str], suffixes: &[&str]) -> bool {
    prefixes.iter().any(|p| s.starts_with(p)) && suffixes.iter().any(|suf| s.contains(suf))
}

fn starts_with_ascii_prefix(s: &str, prefixes: &[&str]) -> bool {
    let lower = s.to_ascii_lowercase();
    prefixes.iter().any(|p| lower.starts_with(p))
}

fn starts_with_cn_list_marker(s: &str) -> bool {
    let numerals = [
        "一、",
        "二、",
        "三、",
        "四、",
        "五、",
        "六、",
        "七、",
        "八、",
        "九、",
        "十、",
        "十一、",
        "十二、",
    ];
    numerals.iter().any(|p| s.starts_with(p))
}

fn leading_number_segments(s: &str) -> usize {
    let mut count = 0usize;
    let chars: Vec<char> = s.trim().chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            break;
        }
        count += 1;
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            continue;
        }
        break;
    }

    count
}

fn is_likely_noise(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return true;
    }
    if t.chars().count() <= 1 {
        return true;
    }
    if t.starts_with("学号")
        || t.starts_with("姓名")
        || t.starts_with("学生姓名")
        || t.starts_with("学院名称")
        || t.starts_with("专业名称")
        || t.starts_with("指导教师")
        || t.starts_with("学院名")
        || t.starts_with("专业名")
        || t.starts_with("学生姓")
        || t.starts_with("指导教")
        || t.starts_with("关键词")
        || t.starts_with("Key words")
    {
        return true;
    }
    if t.chars()
        .all(|c| c.is_ascii_punctuation() || c.is_whitespace())
    {
        return true;
    }
    false
}

fn is_header_footer_noise(text: &str, y: f32) -> bool {
    let t = text.trim();
    if y > 740.0 && t.chars().count() <= 20 {
        return true;
    }
    if y < 80.0 && (looks_like_page_number(t) || looks_like_date_line(t)) {
        return true;
    }
    false
}

fn looks_like_page_number(s: &str) -> bool {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '—');
    !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || c == '/')
}

fn looks_like_date_line(s: &str) -> bool {
    s.contains("年") || s.contains("月") || s.to_ascii_lowercase().contains("202")
}

fn matches_fixed_heading(s: &str) -> bool {
    let t = s.trim();
    [
        "摘要",
        "Abstract",
        "引言",
        "前言",
        "绪论",
        "结论",
        "结束语",
        "参考文献",
        "致谢",
        "附录",
    ]
    .iter()
    .any(|h| t == *h || t.starts_with(&format!("{}{}", h, "：")) || t.starts_with(h))
}

struct CustomPatterns {
    chapter: Option<Regex>,
    section: Option<Regex>,
    subsection: Option<Regex>,
}

impl CustomPatterns {
    fn compile(
        chapter: Option<&str>,
        section: Option<&str>,
        subsection: Option<&str>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            chapter: compile_optional_regex(chapter, "--chapter-pattern")?,
            section: compile_optional_regex(section, "--section-pattern")?,
            subsection: compile_optional_regex(subsection, "--subsection-pattern")?,
        })
    }
}

fn compile_optional_regex(pattern: Option<&str>, flag: &str) -> anyhow::Result<Option<Regex>> {
    match pattern {
        Some(p) => Regex::new(p)
            .map(Some)
            .map_err(|e| anyhow::anyhow!("{} 正则无效: {}", flag, e)),
        None => Ok(None),
    }
}

fn print_heading_preview(doc: &Document, entries: &[OutlineEntry]) {
    if entries.is_empty() {
        println!("（未识别到任何标题）");
        return;
    }
    for entry in entries {
        let page = page_from_ref(doc, entry.page_id).unwrap_or(0);
        let indent = "  ".repeat(entry.level.saturating_sub(1));
        println!("{}[L{}] 第{}页 {}", indent, entry.level, page, entry.title);
    }
}

fn write_outline_tree(
    doc: &mut Document,
    outline_root_id: ObjectId,
    entries: &[OutlineEntry],
) -> anyhow::Result<()> {
    if entries.is_empty() {
        anyhow::bail!("没有可写入的目录项");
    }

    let ids: Vec<ObjectId> = (0..entries.len()).map(|_| next_object_id(doc)).collect();
    let levels: Vec<usize> = entries.iter().map(|e| e.level.max(1)).collect();
    let mut parents = vec![outline_root_id; entries.len()];
    let mut first_child: Vec<Option<usize>> = vec![None; entries.len()];
    let mut last_child: Vec<Option<usize>> = vec![None; entries.len()];
    let mut prev_sibling: Vec<Option<usize>> = vec![None; entries.len()];
    let mut next_sibling: Vec<Option<usize>> = vec![None; entries.len()];
    let mut root_children: Vec<usize> = Vec::new();
    let mut child_counts: Vec<usize> = vec![0; entries.len()];
    let mut stack: Vec<usize> = Vec::new();

    for i in 0..entries.len() {
        while let Some(&last) = stack.last() {
            if levels[last] >= levels[i] {
                stack.pop();
            } else {
                break;
            }
        }

        if let Some(&parent_idx) = stack.last() {
            parents[i] = ids[parent_idx];
            if let Some(prev) = last_child[parent_idx] {
                prev_sibling[i] = Some(prev);
                next_sibling[prev] = Some(i);
            } else {
                first_child[parent_idx] = Some(i);
            }
            last_child[parent_idx] = Some(i);
            child_counts[parent_idx] += 1;
        } else {
            if let Some(&prev_root) = root_children.last() {
                prev_sibling[i] = Some(prev_root);
                next_sibling[prev_root] = Some(i);
            }
            root_children.push(i);
        }

        stack.push(i);
    }

    for i in 0..entries.len() {
        let mut item = lopdf::Dictionary::new();
        item.set("Title", pdf_unicode_string(&entries[i].title));
        item.set("Parent", parents[i]);
        item.set(
            "Dest",
            Object::Array(vec![
                Object::Reference(entries[i].page_id),
                Object::Name(b"Fit".to_vec()),
            ]),
        );
        if let Some(prev) = prev_sibling[i] {
            item.set("Prev", ids[prev]);
        }
        if let Some(next) = next_sibling[i] {
            item.set("Next", ids[next]);
        }
        if let Some(first) = first_child[i] {
            item.set("First", ids[first]);
        }
        if let Some(last) = last_child[i] {
            item.set("Last", ids[last]);
        }
        if child_counts[i] > 0 {
            item.set("Count", child_counts[i] as i64);
        }
        doc.objects.insert(ids[i], Object::Dictionary(item));
    }

    let mut outlines = lopdf::Dictionary::new();
    outlines.set("Type", Object::Name(b"Outlines".to_vec()));
    outlines.set("First", ids[*root_children.first().unwrap()]);
    outlines.set("Last", ids[*root_children.last().unwrap()]);
    outlines.set("Count", root_children.len() as i64);
    doc.objects
        .insert(outline_root_id, Object::Dictionary(outlines));
    Ok(())
}

fn pdf_unicode_string(s: &str) -> Object {
    let mut bytes = vec![0xFE, 0xFF];
    for unit in s.encode_utf16() {
        bytes.push((unit >> 8) as u8);
        bytes.push((unit & 0xFF) as u8);
    }
    Object::String(bytes, lopdf::StringFormat::Hexadecimal)
}

// ─────────────────────────────────────────────────────────────
// sanitize：去除编辑权限 / 展平表单 / 去除交互动作 / 删除签名
// ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn sanitize(
    input: std::path::PathBuf,
    output: std::path::PathBuf,
    remove_perms: bool,
    flatten_forms: bool,
    remove_forms: bool,
    remove_actions: bool,
    remove_sigs: bool,
    remove_annots: bool,
    clean_meta: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    // 默认全部执行
    let all = !remove_perms
        && !flatten_forms
        && !remove_forms
        && !remove_actions
        && !remove_sigs
        && !remove_annots
        && !clean_meta;
    let do_perms = all || remove_perms;
    let do_flatten = all || flatten_forms;
    let do_remove_forms = all || remove_forms;
    let do_actions = all || remove_actions;
    let do_sigs = all || remove_sigs;
    let do_annots = all || remove_annots;
    let do_meta = all || clean_meta;

    let mut doc = crate::util::load_document(&input, password)?;
    let mut report: Vec<String> = Vec::new();

    // ── 1. 去除加密权限 ────────────────────────────────────────
    if do_perms {
        let removed = sanitize_permissions(&mut doc);
        report.push(format!(
            "  权限清理: {}",
            if removed {
                "✅ 已移除 Encrypt / Perms"
            } else {
                "— 无加密字典"
            }
        ));
    }

    // ── 2. 删除所有注释 ───────────────────────────────────────
    if do_annots {
        let mut count = 0usize;
        for pid in doc.get_pages().values().cloned().collect::<Vec<_>>() {
            if let Ok(d) = doc.get_object_mut(pid).and_then(|o| o.as_dict_mut())
                && d.remove(b"Annots").is_some()
            {
                count += 1;
            }
        }
        report.push(format!("  注释删除: ✅ 清除 {} 页的 Annots", count));
    }

    // ── 3. 展平表单（优先）或直接删除表单 ──────────────────────
    if do_flatten {
        let n = sanitize_flatten_forms(&mut doc)?;
        report.push(format!("  表单展平: ✅ 处理 {} 个字段，AcroForm 已移除", n));
    } else if do_remove_forms
        && let Ok(root_id) = doc.trailer.get(b"Root").and_then(|o| o.as_reference())
        && let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut())
    {
        if root.remove(b"AcroForm").is_some() {
            report.push("  表单删除: ✅ AcroForm 已直接删除".to_string());
        } else {
            report.push("  表单删除: — 无 AcroForm".to_string());
        }
    }

    // ── 4. 去除页面 / 目录交互动作 ────────────────────────────
    if do_actions {
        let n = sanitize_remove_actions(&mut doc)?;
        report.push(format!("  交互动作: ✅ 清理 {} 处动作/触发器", n));
    }

    // ── 5. 删除数字签名 ───────────────────────────────────────
    if do_sigs {
        let n = sanitize_remove_signatures(&mut doc)?;
        report.push(format!("  数字签名: ✅ 移除 {} 个签名字段/注释", n));
    }

    // ── 6. 清理元数据 ─────────────────────────────────────────
    if do_meta {
        doc.trailer.remove(b"Info");
        if let Ok(root_id) = doc.trailer.get(b"Root").and_then(|o| o.as_reference())
            && let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut())
        {
            root.remove(b"Metadata");
            root.remove(b"Permissions");
        }
        report.push("  元数据:   ✅ Info / Metadata / Permissions 已清除".to_string());
    }

    crate::util::save_document(&mut doc, output)?;
    println!("✨ sanitize 完成：");
    for line in &report {
        println!("{}", line);
    }
    Ok(())
}

// ── 实现：去除加密权限 ────────────────────────────────────────

fn sanitize_permissions(doc: &mut Document) -> bool {
    if doc.trailer.get(b"Encrypt").is_err() {
        return false;
    }
    // 移除 Encrypt 间接对象
    if let Ok(enc_id) = doc.trailer.get(b"Encrypt").and_then(|o| o.as_reference()) {
        doc.objects.remove(&enc_id);
    }
    doc.trailer.remove(b"Encrypt");
    // 同时清理 Catalog 中 Permissions / Perms
    if let Ok(root_id) = doc.trailer.get(b"Root").and_then(|o| o.as_reference())
        && let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut())
    {
        root.remove(b"Perms");
        root.remove(b"Permissions");
    }
    true
}

// ── 实现：展平 AcroForm 表单 ─────────────────────────────────
//
// 策略：遍历 AcroForm/Fields 数组中每个字段，取其 /AP /N（正常外观）
// 流，把流内容追加到对应页面的内容流末尾，然后删除 AcroForm。
// 对于没有 AP 的字段，仅删除注释引用（使其不可交互）。

fn sanitize_flatten_forms(doc: &mut Document) -> anyhow::Result<usize> {
    let root_id = doc.trailer.get(b"Root")?.as_reference()?;

    // 提前收集所有字段 ID
    let field_ids: Vec<ObjectId> = {
        let root = doc.get_object(root_id)?.as_dict()?;
        let acro = match root.get(b"AcroForm") {
            Ok(o) => o.clone(),
            Err(_) => return Ok(0),
        };
        let acro_dict = if let Ok(id) = acro.as_reference() {
            doc.get_object(id)?.as_dict()?.clone()
        } else {
            acro.as_dict()?.clone()
        };
        collect_field_ids(doc, &acro_dict)?
    };

    let mut count = 0usize;

    // 建立 annotation → page_id 映射（用于定位字段所在页）
    let page_annot_map = build_page_annot_map(doc)?;

    for field_id in &field_ids {
        // 取 AP/N 外观流
        let ap_content: Option<Vec<u8>> = {
            let field_obj = doc.get_object(*field_id);
            if let Ok(field_dict) = field_obj.and_then(|o| o.as_dict()) {
                if let Ok(ap) = field_dict.get(b"AP").and_then(|o| o.as_dict()) {
                    if let Ok(n_ref) = ap.get(b"N").and_then(|o| o.as_reference()) {
                        if let Ok(stream) = doc.get_object(n_ref).and_then(|o| o.as_stream()) {
                            stream.decompressed_content().ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };

        // 找到该字段所在页面
        if let Some(ap_bytes) = ap_content
            && let Some(&page_id) = page_annot_map.get(field_id)
        {
            // 在页面内容流末尾追加外观内容（用 q/Q 包裹隔离图形状态）
            let mut existing = doc.get_page_content(page_id).unwrap_or_default();
            existing.extend_from_slice(b"\nq\n");
            existing.extend_from_slice(&ap_bytes);
            existing.extend_from_slice(b"\nQ\n");
            doc.change_page_content(page_id, existing)?;
            count += 1;
        }

        // 从页面 Annots 数组中删除该字段引用
        if let Some(&page_id) = page_annot_map.get(field_id) {
            remove_annot_ref(doc, page_id, *field_id);
        }
    }

    // 删除 AcroForm
    if let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut()) {
        root.remove(b"AcroForm");
    }

    Ok(count)
}

fn collect_field_ids(
    doc: &Document,
    acro_dict: &lopdf::Dictionary,
) -> anyhow::Result<Vec<ObjectId>> {
    let mut ids = Vec::new();
    if let Ok(fields_obj) = acro_dict.get(b"Fields") {
        let arr = match fields_obj {
            Object::Array(a) => a.clone(),
            Object::Reference(r) => doc.get_object(*r)?.as_array()?.clone(),
            _ => return Ok(ids),
        };
        for item in &arr {
            if let Ok(id) = item.as_reference() {
                ids.push(id);
                // 递归收集子字段
                if let Ok(child) = doc.get_object(id).and_then(|o| o.as_dict())
                    && child.get(b"Kids").is_ok()
                {
                    let sub = collect_field_ids(doc, child)?;
                    ids.extend(sub);
                }
            }
        }
    }
    Ok(ids)
}

/// page_id → annotation ObjectId 的反向映射
fn build_page_annot_map(
    doc: &Document,
) -> anyhow::Result<std::collections::HashMap<ObjectId, ObjectId>> {
    let mut map = std::collections::HashMap::new();
    for &page_id in doc.get_pages().values() {
        if let Ok(page_dict) = doc.get_object(page_id).and_then(|o| o.as_dict())
            && let Ok(annots) = page_dict.get(b"Annots").and_then(|o| o.as_array())
        {
            for ann in annots {
                if let Ok(ann_id) = ann.as_reference() {
                    map.insert(ann_id, page_id);
                }
            }
        }
    }
    Ok(map)
}

fn remove_annot_ref(doc: &mut Document, page_id: ObjectId, target: ObjectId) {
    if let Ok(page_dict) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut())
        && let Ok(annots) = page_dict.get_mut(b"Annots").and_then(|o| o.as_array_mut())
    {
        annots.retain(|o| o.as_reference().ok() != Some(target));
    }
}

// ── 实现：去除交互动作 ────────────────────────────────────────

fn sanitize_remove_actions(doc: &mut Document) -> anyhow::Result<usize> {
    let mut count = 0usize;

    // Catalog 级：AA / OpenAction / URI
    if let Ok(root_id) = doc.trailer.get(b"Root").and_then(|o| o.as_reference())
        && let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut())
    {
        for key in &[b"AA".as_ref(), b"OpenAction", b"URI"] {
            if root.remove(key).is_some() {
                count += 1;
            }
        }
    }

    // 页面级：AA / OpenAction；注释级：A / AA（保留无动作注释）
    let page_ids: Vec<ObjectId> = doc.get_pages().values().cloned().collect();
    for page_id in page_ids {
        // 页面字典自身的 AA
        {
            if let Ok(page_dict) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
                for key in &[b"AA".as_ref(), b"OpenAction"] {
                    if page_dict.remove(key).is_some() {
                        count += 1;
                    }
                }
            }
        }
        // 收集注释 ID
        let annot_ids: Vec<ObjectId> = {
            if let Ok(page_dict) = doc.get_object(page_id).and_then(|o| o.as_dict()) {
                if let Ok(annots) = page_dict.get(b"Annots").and_then(|o| o.as_array()) {
                    annots
                        .iter()
                        .filter_map(|o| o.as_reference().ok())
                        .collect()
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        };
        for ann_id in annot_ids {
            if let Ok(ann) = doc.get_object_mut(ann_id).and_then(|o| o.as_dict_mut()) {
                for key in &[b"A".as_ref(), b"AA"] {
                    if ann.remove(key).is_some() {
                        count += 1;
                    }
                }
            }
        }
    }
    Ok(count)
}

// ── 实现：删除数字签名 ────────────────────────────────────────

fn sanitize_remove_signatures(doc: &mut Document) -> anyhow::Result<usize> {
    let mut count = 0usize;
    let root_id = doc.trailer.get(b"Root")?.as_reference()?;

    // 收集 AcroForm 中 /Sig 类型的字段 ID
    let sig_field_ids: Vec<ObjectId> = {
        let root = doc.get_object(root_id)?.as_dict()?;
        let acro_obj = match root.get(b"AcroForm") {
            Ok(o) => o.clone(),
            Err(_) => return Ok(0),
        };
        let acro_dict = if let Ok(id) = acro_obj.as_reference() {
            doc.get_object(id)?.as_dict()?.clone()
        } else {
            acro_obj.as_dict()?.clone()
        };
        let all_fields = collect_field_ids(doc, &acro_dict)?;
        let mut sigs = Vec::new();
        for id in all_fields {
            let is_sig = {
                if let Ok(d) = doc.get_object(id).and_then(|o| o.as_dict()) {
                    d.get(b"FT").and_then(|v| v.as_name()).ok() == Some(b"Sig")
                } else {
                    false
                }
            };
            if is_sig {
                sigs.push(id);
            }
        }
        sigs
    };

    let page_annot_map = build_page_annot_map(doc)?;

    for sig_id in &sig_field_ids {
        // 删除关联的 /V (Sig 值流对象)
        let v_id: Option<ObjectId> = doc
            .get_object(*sig_id)
            .and_then(|o| o.as_dict())
            .ok()
            .and_then(|d| d.get(b"V").and_then(|o| o.as_reference()).ok());
        if let Some(vid) = v_id {
            doc.objects.remove(&vid);
        }
        // 从页面 Annots 移除引用
        if let Some(&page_id) = page_annot_map.get(sig_id) {
            remove_annot_ref(doc, page_id, *sig_id);
        }
        // 删除字段对象
        doc.objects.remove(sig_id);
        count += 1;
    }

    // 从 AcroForm Fields 数组中移除这些 ID
    if !sig_field_ids.is_empty() {
        let acro_obj_id: Option<ObjectId> = doc
            .get_object(root_id)
            .and_then(|o| o.as_dict())
            .ok()
            .and_then(|d| d.get(b"AcroForm").and_then(|o| o.as_reference()).ok());
        if let Some(acro_id) = acro_obj_id {
            if let Ok(acro) = doc.get_object_mut(acro_id).and_then(|o| o.as_dict_mut()) {
                acro.remove(b"SigFlags");
                if let Ok(fields) = acro.get_mut(b"Fields").and_then(|o| o.as_array_mut()) {
                    fields.retain(|o| {
                        o.as_reference()
                            .map(|id| !sig_field_ids.contains(&id))
                            .unwrap_or(true)
                    });
                }
            }
        } else if let Ok(root) = doc.get_object_mut(root_id).and_then(|o| o.as_dict_mut()) {
            // AcroForm 是内联字典
            if let Ok(acro) = root.get_mut(b"AcroForm").and_then(|o| o.as_dict_mut()) {
                acro.remove(b"SigFlags");
                if let Ok(fields) = acro.get_mut(b"Fields").and_then(|o| o.as_array_mut()) {
                    fields.retain(|o| {
                        o.as_reference()
                            .map(|id| !sig_field_ids.contains(&id))
                            .unwrap_or(true)
                    });
                }
            }
        }
    }

    Ok(count)
}
