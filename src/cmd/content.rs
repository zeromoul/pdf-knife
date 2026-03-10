use crate::util::{
    category_for_op, fmt_operand, is_pdf_operator, ops_for_category, tokenize_stream, unescape,
};
use lopdf::{Document, content::Content};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn extract(
    input: PathBuf,
    page: u32,
    pages: Option<String>,
    all_pages: bool,
    output: Option<PathBuf>,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;
    let pages_to_run = crate::util::select_pages(&doc, page, &pages, all_pages)?;
    let multi = pages_to_run.len() > 1;
    if multi && output.is_some() {
        anyhow::bail!("批量页面模式下不支持 --output，请省略该参数以输出到 stdout");
    }
    for cur_page in &pages_to_run {
        let page_id = get_page_id(&doc, *cur_page)?;
        let data = doc.get_page_content(page_id)?;
        // 内容流通常是混杂操作符和编码文本的二进制数据，转为 UTF-8 仅供参考。
        let text = String::from_utf8_lossy(&data);
        if let Some(ref path) = output {
            std::fs::write(path, &data)?;
            println!("✅ 原始内容流（二进制）已导出。");
        } else {
            println!(
                "\n====== 第 {} 页 内容流 ({} 字节) ======",
                cur_page,
                data.len()
            );
            println!("{}", text);
        }
    }
    Ok(())
}

pub fn import(
    input_pdf: PathBuf,
    output_pdf: PathBuf,
    page: u32,
    stream_file: PathBuf,
    strip_whitespace: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let mut doc = crate::util::load_document(&input_pdf, password)?;
    let page_id = get_page_id(&doc, page)?;
    let mut content = std::fs::read_to_string(&stream_file)?;
    if strip_whitespace {
        content = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
    doc.change_page_content(page_id, content.into_bytes())?;
    crate::util::save_document(&mut doc, output_pdf)?;
    println!("✅ 内容流导入完成。");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn list_ops(
    input: PathBuf,
    page: u32,
    category: Option<String>,
    op_filter: Vec<String>,
    stats: bool,
    offsets: bool,
    xobject: Option<String>,
    all_pages: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;

    // 确定要处理的页面列表
    let pages_to_run: Vec<u32> = if all_pages {
        let mut ps: Vec<u32> = doc.get_pages().keys().cloned().collect();
        ps.sort();
        ps
    } else {
        vec![page]
    };

    for cur_page in pages_to_run {
        let page_id = get_page_id(&doc, cur_page)?;

        // 如果指定了 --xobject，解析该页上某个 Form XObject 的内容流
        let raw = if let Some(ref xobj_name) = xobject {
            let resources = crate::util::resolve_resources(&doc, page_id)?;
            let xobjs = resources.get(b"XObject")?.as_dict()?;
            let xobj_id = xobjs.get(xobj_name.as_bytes())?.as_reference()?;
            let stream = doc.get_object(xobj_id)?.as_stream()?;
            let subtype = stream
                .dict
                .get(b"Subtype")
                .and_then(|v| v.as_name())
                .unwrap_or(b"");
            if subtype != b"Form" {
                anyhow::bail!(
                    "/{} 不是 Form XObject（实际 Subtype: /{}）",
                    xobj_name,
                    String::from_utf8_lossy(subtype)
                );
            }
            println!("\n--- Form XObject /{} 的内部操作符 ---", xobj_name);
            stream.decompressed_content()?
        } else {
            doc.get_page_content(page_id)?
        };

        let content = Content::decode(&raw)?;
        let total = content.operations.len();

        if all_pages && xobject.is_none() {
            println!("\n====== 第 {} 页 ======", cur_page);
        }

        if offsets {
            offsets_view(&raw, cur_page, &category, &op_filter)?;
            continue;
        }

        let cat_ops: Option<Vec<&str>> = category.as_deref().map(ops_for_category);

        if stats {
            let mut counts: HashMap<&str, usize> = HashMap::new();
            for op in &content.operations {
                *counts.entry(op.operator.as_str()).or_insert(0) += 1;
            }
            let mut sorted: Vec<_> = counts.iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            println!("\n--- 第 {} 页操作符统计 (共 {} 个) ---", cur_page, total);
            println!("{:<10} | {:<6} | 分类", "操作符", "次数");
            println!("{}", "-".repeat(32));
            for (name, count) in &sorted {
                println!("{:<10} | {:<6} | {}", name, count, category_for_op(name));
            }
        } else {
            let mut printed = 0usize;
            for (i, op) in content.operations.iter().enumerate() {
                if let Some(ref f) = cat_ops
                    && !f.contains(&op.operator.as_str())
                {
                    continue;
                }
                if !op_filter.is_empty() && !op_filter.iter().any(|o| o == &op.operator) {
                    continue;
                }
                let ops_str = op
                    .operands
                    .iter()
                    .map(fmt_operand)
                    .collect::<Vec<_>>()
                    .join("  ");
                println!("{:<5} | {:<10} | {}", i, op.operator, ops_str);
                printed += 1;
            }
            println!("\n--- 共输出 {} / {} 个操作符 ---", printed, total);
        }

        // xobject 只处理一次
        if xobject.is_some() {
            break;
        }
    }
    Ok(())
}

fn offsets_view(
    raw: &[u8],
    page: u32,
    category: &Option<String>,
    op_filter: &[String],
) -> anyhow::Result<()> {
    let tokens = tokenize_stream(raw);
    let mut groups: Vec<Vec<(usize, usize, Vec<u8>)>> = Vec::new();
    let mut current: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    for tok in tokens {
        let is_op = is_pdf_operator(&tok.2);
        current.push(tok);
        if is_op {
            groups.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }

    for (idx, group) in groups.iter().enumerate() {
        let op_name = group
            .last()
            .map(|(_, _, b)| String::from_utf8_lossy(b).to_string())
            .unwrap_or_default();

        if let Some(cat) = category {
            let allowed = ops_for_category(cat);
            if !allowed.is_empty() && !allowed.contains(&op_name.as_str()) {
                continue;
            }
        }
        if !op_filter.is_empty() && !op_filter.iter().any(|o| o == &op_name) {
            continue;
        }

        let col1 = format!("{:04}", idx + 1);
        let col2 = group
            .iter()
            .map(|(_, _, b)| String::from_utf8_lossy(b).into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        let col3 = group
            .iter()
            .map(|(s, e, _)| format!("offset({},{})", s, e))
            .collect::<Vec<_>>()
            .join(" --> ");
        println!("{} | {} | {}", col1, col2, col3);
    }
    println!("\n--- 第 {} 页共 {} 个操作 ---", page, groups.len());
    Ok(())
}

pub fn delete_ops(
    input: PathBuf,
    output: PathBuf,
    page: u32,
    range: Vec<usize>,
    skip: Vec<usize>,
    password: &Option<String>,
) -> anyhow::Result<()> {
    if range.len() < 2 {
        anyhow::bail!("--range 需要提供恰好 2 个值，例如: --range 10 20");
    }
    let (rs, re) = (range[0], range[1]);
    if rs > re {
        anyhow::bail!("--range 起始值 ({}) 不能大于结束值 ({})", rs, re);
    }
    let mut doc = crate::util::load_document(&input, password)?;
    let page_id = get_page_id(&doc, page)?;
    let raw = doc.get_page_content(page_id)?;
    let mut content = Content::decode(&raw)?;
    let total = content.operations.len();
    if re >= total {
        anyhow::bail!("--range 结束值 ({}) 超出操作符总数 ({})", re, total);
    }
    content.operations = content
        .operations
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !(*i >= rs && *i <= re && !skip.contains(i)))
        .map(|(_, op)| op)
        .collect();
    doc.change_page_content(page_id, content.encode()?)?;
    crate::util::save_document(&mut doc, output)?;
    println!("✅ 已删除序号 {}~{} 范围的操作符。", rs, re);
    Ok(())
}

pub fn replace_content(
    input: PathBuf,
    output: PathBuf,
    page: u32,
    old: String,
    new: String,
    use_regex: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let mut doc = crate::util::load_document(&input, password)?;
    let page_id = get_page_id(&doc, page)?;
    let raw = doc.get_page_content(page_id)?;
    let mut content = Content::decode(&raw)?;
    let (o, n) = (unescape(&old), unescape(&new));
    let mut count = 0;

    for op in &mut content.operations {
        if op.operator == "Tj" || op.operator == "TJ" {
            for operand in &mut op.operands {
                match operand {
                    lopdf::Object::String(s, _) => {
                        let text = String::from_utf8_lossy(s).to_string();
                        let updated = if use_regex {
                            regex::Regex::new(&o)?.replace_all(&text, &n).to_string()
                        } else {
                            text.replace(&o, &n)
                        };
                        if updated != text {
                            *s = updated.into_bytes();
                            count += 1;
                        }
                    }
                    lopdf::Object::Array(arr) => {
                        for item in arr {
                            if let lopdf::Object::String(s, _) = item {
                                let text = String::from_utf8_lossy(s).to_string();
                                let updated = if use_regex {
                                    regex::Regex::new(&o)?.replace_all(&text, &n).to_string()
                                } else {
                                    text.replace(&o, &n)
                                };
                                if updated != text {
                                    *s = updated.into_bytes();
                                    count += 1;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if count > 0 {
        doc.change_page_content(page_id, content.encode()?)?;
        crate::util::save_document(&mut doc, output)?;
        println!(
            "✅ 替换完成，共修改 {} 处文本。 (注：非 UTF-8 编码的文本可能无法匹配)",
            count
        );
    } else {
        println!("⚠️ 未找到匹配的文本。");
    }
    Ok(())
}

pub fn hex_view(
    input: PathBuf,
    page: u32,
    pages: Option<String>,
    all_pages: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;
    let pages_to_run = crate::util::select_pages(&doc, page, &pages, all_pages)?;
    let multi = pages_to_run.len() > 1;
    for cur_page in &pages_to_run {
        let page_id = get_page_id(&doc, *cur_page)?;
        let data = doc.get_page_content(page_id)?;
        if multi {
            println!("\n====== 第 {} 页 ======", cur_page);
        }
        println!("\n偏移       | 十六进制                                         | ASCII");
        println!("{}", "-".repeat(75));
        for (i, chunk) in data.chunks(16).enumerate() {
            let hex = chunk
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");
            let ascii: String = chunk
                .iter()
                .map(|&b| {
                    if (32..=126).contains(&b) {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            println!("{:08X}   | {:<47} | {}", i * 16, hex, ascii);
        }
    }
    Ok(())
}

fn get_page_id(doc: &Document, page: u32) -> anyhow::Result<lopdf::ObjectId> {
    doc.get_pages()
        .get(&page)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("页码 {} 超出范围", page))
}

// ─────────────────────────────────────────────────────────────
// 通用目标流抽象：页面内容流 / 任意对象流（Form XObject 等）
// ─────────────────────────────────────────────────────────────

enum ContentTarget {
    Page(lopdf::ObjectId),
    Stream(lopdf::ObjectId),
}

/// 根据用户参数解析目标（obj_id 优先，其次 xobject，最后 page 内容流）
fn resolve_target(
    doc: &Document,
    page: u32,
    obj_id: Option<u32>,
    xobject: &Option<String>,
) -> anyhow::Result<ContentTarget> {
    if let Some(num) = obj_id {
        let oid = doc
            .objects
            .keys()
            .find(|(n, _)| *n == num)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("未找到对象 ID = {}", num))?;
        return Ok(ContentTarget::Stream(oid));
    }
    let page_id = get_page_id(doc, page)?;
    if let Some(xname) = xobject {
        let resources = crate::util::resolve_resources(doc, page_id)?;
        let xobjs = resources.get(b"XObject")?.as_dict()?;
        let xobj_id = xobjs.get(xname.as_bytes())?.as_reference()?;
        return Ok(ContentTarget::Stream(xobj_id));
    }
    Ok(ContentTarget::Page(page_id))
}

fn read_target(doc: &Document, target: &ContentTarget) -> anyhow::Result<Vec<u8>> {
    match target {
        ContentTarget::Page(pid) => Ok(doc.get_page_content(*pid)?),
        ContentTarget::Stream(oid) => {
            Ok(doc.get_object(*oid)?.as_stream()?.decompressed_content()?)
        }
    }
}

fn write_target(doc: &mut Document, target: &ContentTarget, data: Vec<u8>) -> anyhow::Result<()> {
    match target {
        ContentTarget::Page(pid) => doc.change_page_content(*pid, data)?,
        ContentTarget::Stream(oid) => {
            let obj = doc.get_object_mut(*oid)?;
            match obj {
                lopdf::Object::Stream(stream) => {
                    stream.dict.remove(b"Filter");
                    stream.dict.remove(b"DecodeParms");
                    stream.content = data;
                }
                _ => anyhow::bail!("对象 {} 不是流对象", oid.0),
            }
        }
    }
    Ok(())
}

/// 将命令行字符串解析为 lopdf::Object 操作数
///   /Name  → Name
///   1 / 0.5 → Integer / Real
///   true/false/null → Boolean/Null
///   其余（含 (text)） → Literal String
pub fn parse_operand(s: &str) -> lopdf::Object {
    if let Some(name) = s.strip_prefix('/') {
        return lopdf::Object::Name(name.as_bytes().to_vec());
    }
    match s {
        "true" => return lopdf::Object::Boolean(true),
        "false" => return lopdf::Object::Boolean(false),
        "null" => return lopdf::Object::Null,
        _ => {}
    }
    if let Ok(n) = s.parse::<i64>() {
        return lopdf::Object::Integer(n);
    }
    if let Ok(f) = s.parse::<f32>() {
        return lopdf::Object::Real(f);
    }
    // 字符串，剥去可选的括号
    let text = if s.starts_with('(') && s.ends_with(')') && s.len() >= 2 {
        s[1..s.len() - 1].to_owned()
    } else {
        s.to_owned()
    };
    lopdf::Object::String(text.into_bytes(), lopdf::StringFormat::Literal)
}

// ─────────────────────────────────────────────────────────────
// patch-op：修改指定序号操作符的操作数
// ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn patch_op(
    input: PathBuf,
    output: PathBuf,
    page: u32,
    obj_id: Option<u32>,
    index: usize,
    operator: Option<String>,
    operands: Vec<String>,
    xobject: Option<String>,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let mut doc = crate::util::load_document(&input, password)?;
    let target = resolve_target(&doc, page, obj_id, &xobject)?;
    let raw = read_target(&doc, &target)?;

    let mut content = Content::decode(&raw)?;
    let total = content.operations.len();
    if index >= total {
        anyhow::bail!(
            "操作符序号 {} 超出范围（共 {} 个，序号从 0 起）",
            index,
            total
        );
    }

    // 可选：校验操作符名称，防止误操作
    if let Some(ref expected) = operator {
        let actual = &content.operations[index].operator;
        if actual != expected {
            anyhow::bail!(
                "序号 {} 处的操作符为 '{}', 与指定的 '{}' 不符（如要强制执行，去掉 --operator）",
                index,
                actual,
                expected
            );
        }
    }

    let op_name = content.operations[index].operator.clone();
    let before: Vec<String> = content.operations[index]
        .operands
        .iter()
        .map(crate::util::fmt_operand)
        .collect();

    content.operations[index].operands = operands.iter().map(|s| parse_operand(s)).collect();

    let after: Vec<String> = content.operations[index]
        .operands
        .iter()
        .map(crate::util::fmt_operand)
        .collect();

    println!("操作符: {} (序号 {})", op_name, index);
    println!("  修改前: {}", before.join("  "));
    println!("  修改后: {}", after.join("  "));

    write_target(&mut doc, &target, content.encode()?)?;
    crate::util::save_document(&mut doc, output)?;
    println!("✅ 操作符修改完成。");
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// insert-op：在指定位置插入新操作符
// ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn insert_op(
    input: PathBuf,
    output: PathBuf,
    page: u32,
    obj_id: Option<u32>,
    index: usize,
    operator: String,
    operands: Vec<String>,
    xobject: Option<String>,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let mut doc = crate::util::load_document(&input, password)?;
    let target = resolve_target(&doc, page, obj_id, &xobject)?;
    let raw = read_target(&doc, &target)?;

    let mut content = Content::decode(&raw)?;
    let total = content.operations.len();
    let insert_at = index.min(total); // 超范围自动追加到末尾

    let new_op = lopdf::content::Operation {
        operator: operator.clone(),
        operands: operands.iter().map(|s| parse_operand(s)).collect(),
    };

    content.operations.insert(insert_at, new_op);

    println!("插入 #{}: {}  {}", insert_at, operands.join("  "), operator);
    println!("  原共 {} 个操作符，现共 {} 个。", total, total + 1);

    write_target(&mut doc, &target, content.encode()?)?;
    crate::util::save_document(&mut doc, output)?;
    println!("✅ 操作符插入完成。");
    Ok(())
}
