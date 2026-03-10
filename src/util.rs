use lopdf::{Dictionary, Document, Object, ObjectId};

/// 解转义 \n \r \t \\
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(&'n') => {
                    out.push('\n');
                    chars.next();
                }
                Some(&'r') => {
                    out.push('\r');
                    chars.next();
                }
                Some(&'t') => {
                    out.push('\t');
                    chars.next();
                }
                Some(&'\\') => {
                    out.push('\\');
                    chars.next();
                }
                _ => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// 对 PDF 内容流进行字节级 tokenize，返回 (start, end, token_bytes)
pub fn tokenize_stream(data: &[u8]) -> Vec<(usize, usize, Vec<u8>)> {
    let mut tokens: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    let mut i = 0;
    while i < data.len() {
        loop {
            while i < data.len()
                && matches!(data[i], b' ' | b'\t' | b'\r' | b'\n' | b'\x0C' | b'\0')
            {
                i += 1;
            }
            if i < data.len() && data[i] == b'%' {
                while i < data.len() && data[i] != b'\n' && data[i] != b'\r' {
                    i += 1;
                }
            } else {
                break;
            }
        }
        if i >= data.len() {
            break;
        }
        let start = i;
        match data[i] {
            b'(' => {
                i += 1;
                let mut depth = 1i32;
                while i < data.len() && depth > 0 {
                    if data[i] == b'\\' {
                        i += 1;
                        if i < data.len() {
                            i += 1;
                        }
                        continue;
                    }
                    if data[i] == b'(' {
                        depth += 1;
                    } else if data[i] == b')' {
                        depth -= 1;
                    }
                    i += 1;
                }
                tokens.push((start, i, data[start..i].to_vec()));
            }
            b'<' if i + 1 < data.len() && data[i + 1] == b'<' => {
                i += 2;
                let mut depth = 1i32;
                while i + 1 < data.len() && depth > 0 {
                    if data[i] == b'<' && data[i + 1] == b'<' {
                        depth += 1;
                        i += 2;
                    } else if data[i] == b'>' && data[i + 1] == b'>' {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                tokens.push((start, i, data[start..i].to_vec()));
            }
            b'<' => {
                i += 1;
                while i < data.len() && data[i] != b'>' {
                    i += 1;
                }
                if i < data.len() {
                    i += 1;
                }
                tokens.push((start, i, data[start..i].to_vec()));
            }
            b'[' => {
                i += 1;
                let mut depth = 1i32;
                while i < data.len() && depth > 0 {
                    match data[i] {
                        b'[' => {
                            depth += 1;
                            i += 1;
                        }
                        b']' => {
                            depth -= 1;
                            i += 1;
                        }
                        b'(' => {
                            i += 1;
                            let mut sd = 1i32;
                            while i < data.len() && sd > 0 {
                                if data[i] == b'\\' {
                                    i += 1;
                                    if i < data.len() {
                                        i += 1;
                                    }
                                    continue;
                                }
                                if data[i] == b'(' {
                                    sd += 1;
                                } else if data[i] == b')' {
                                    sd -= 1;
                                }
                                i += 1;
                            }
                        }
                        _ => {
                            i += 1;
                        }
                    }
                }
                tokens.push((start, i, data[start..i].to_vec()));
            }
            _ => {
                while i < data.len()
                    && !matches!(
                        data[i],
                        b' ' | b'\t'
                            | b'\r'
                            | b'\n'
                            | b'\x0C'
                            | b'\0'
                            | b'('
                            | b')'
                            | b'<'
                            | b'>'
                            | b'['
                            | b']'
                            | b'{'
                            | b'}'
                            | b'%'
                    )
                {
                    i += 1;
                }
                if i > start {
                    tokens.push((start, i, data[start..i].to_vec()));
                }
            }
        }
    }
    tokens
}

/// 判断 token 是否为 PDF 操作符（而非操作数）
pub fn is_pdf_operator(tok: &[u8]) -> bool {
    if tok.is_empty() {
        return false;
    }
    match tok[0] {
        b'0'..=b'9' | b'+' | b'-' | b'.' | b'(' | b'<' | b'[' | b'/' => false,
        _ => std::str::from_utf8(tok)
            .map(|s| s.parse::<f64>().is_err())
            .unwrap_or(true),
    }
}

/// 按 16 类 PDF 操作符分类返回操作符列表
pub fn ops_for_category(cat: &str) -> Vec<&'static str> {
    match cat {
        "gstate" => vec!["w", "J", "j", "M", "d", "ri", "i", "gs"],
        "special" => vec!["q", "Q", "cm"],
        "path" => vec!["m", "l", "c", "v", "y", "h", "re"],
        "paint" => vec!["S", "s", "f", "F", "f*", "B", "B*", "b", "b*", "n"],
        "clip" => vec!["W", "W*"],
        "textobj" => vec!["BT", "ET"],
        "textstate" => vec!["Tc", "Tw", "Tz", "TL", "Tf", "Tr", "Ts"],
        "textpos" => vec!["Td", "TD", "Tm", "T*"],
        "textshow" => vec!["Tj", "TJ", "'", "\""],
        "type3" => vec!["d0", "d1"],
        "color" => vec![
            "CS", "cs", "SC", "SCN", "sc", "scn", "G", "g", "RG", "rg", "K", "k",
        ],
        "shading" => vec!["sh"],
        "inline" => vec!["BI", "ID", "EI"],
        "xobject" => vec!["Do"],
        "marked" => vec!["MP", "DP", "BMC", "BDC", "EMC"],
        "compat" => vec!["BX", "EX"],
        _ => vec![],
    }
}

/// 返回操作符所属的 16 类分类名
pub fn category_for_op(op: &str) -> &'static str {
    match op {
        "w" | "J" | "j" | "M" | "d" | "ri" | "i" | "gs" => "gstate",
        "q" | "Q" | "cm" => "special",
        "m" | "l" | "c" | "v" | "y" | "h" | "re" => "path",
        "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => "paint",
        "W" | "W*" => "clip",
        "BT" | "ET" => "textobj",
        "Tc" | "Tw" | "Tz" | "TL" | "Tf" | "Tr" | "Ts" => "textstate",
        "Td" | "TD" | "Tm" | "T*" => "textpos",
        "Tj" | "TJ" | "'" | "\"" => "textshow",
        "d0" | "d1" => "type3",
        "CS" | "cs" | "SC" | "SCN" | "sc" | "scn" | "G" | "g" | "RG" | "rg" | "K" | "k" => "color",
        "sh" => "shading",
        "BI" | "ID" | "EI" => "inline",
        "Do" => "xobject",
        "MP" | "DP" | "BMC" | "BDC" | "EMC" => "marked",
        "BX" | "EX" => "compat",
        _ => "other",
    }
}

/// 解引用 Resources 字典（可能是直接字典或间接引用）
pub fn resolve_resources(doc: &Document, page_id: ObjectId) -> anyhow::Result<Dictionary> {
    let page_dict = doc.get_object(page_id)?.as_dict()?;
    let res_obj = page_dict.get(b"Resources")?;
    Ok(if let Ok(id) = res_obj.as_reference() {
        doc.get_object(id)?.as_dict()?.clone()
    } else {
        res_obj.as_dict()?.clone()
    })
}

/// 将 Object 格式化为可读字符串（操作数显示用）
pub fn fmt_operand(o: &Object) -> String {
    match o {
        Object::Integer(n) => n.to_string(),
        Object::Real(f) => format!("{:.3}", f),
        Object::Name(n) => format!("/{}", String::from_utf8_lossy(n)),
        Object::String(_, _) => {
            let decoded = lopdf::decode_text_string(o).unwrap_or_else(|_| {
                if let Ok(s) = o.as_str() {
                    String::from_utf8_lossy(s).to_string()
                } else {
                    "(err)".into()
                }
            });
            format!("({})", decoded)
        }
        Object::Array(a) => format!("[..{}项]", a.len()),
        other => format!("{:?}", other),
    }
}

/// 加载 PDF 文档，支持密码（如果提供）
pub fn load_document(
    path: &std::path::Path,
    password: &Option<String>,
) -> anyhow::Result<Document> {
    if let Some(pwd) = password {
        Ok(Document::load_with_password(path, pwd)?)
    } else {
        Ok(Document::load(path)?)
    }
}

/// 以安全且兼容的方式保存 PDF 文档（仅压缩流内容，不改变对象 ID）
pub fn save_document(doc: &mut Document, path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
    // 1. 仅压缩流内容（如文字、图片流），这不涉及结构改变
    doc.compress();

    // 2. 配置保存选项：禁用会导致重编号的对象流，但开启最大压缩
    let options = lopdf::SaveOptions::builder()
        .use_object_streams(false)
        .use_xref_streams(false)
        .compression_level(9)
        .build();

    let mut file = std::fs::File::create(path)?;
    doc.save_with_options(&mut file, options)?;
    Ok(())
}

/// 解析页码字符串，如 "1,3,5-10" 返回 vec![1,3,5,6,7,8,9,10]
pub fn parse_pages(s: &str) -> anyhow::Result<Vec<u32>> {
    let mut result = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let range: Vec<&str> = part.split('-').collect();
            if range.len() != 2 {
                anyhow::bail!("页码范围格式错误: {}", part);
            }
            let start: u32 = range[0].trim().parse()?;
            let end: u32 = range[1].trim().parse()?;
            if start > end {
                anyhow::bail!("页码范围起点大于终点: {}", part);
            }
            for p in start..=end {
                result.push(p);
            }
        } else {
            result.push(part.parse()?);
        }
    }
    result.sort_unstable();
    result.dedup();
    Ok(result)
}

/// 根据 page / pages / all_pages 解析要处理的页码列表
pub fn select_pages(
    doc: &Document,
    page: u32,
    pages: &Option<String>,
    all_pages: bool,
) -> anyhow::Result<Vec<u32>> {
    if all_pages && pages.is_some() {
        anyhow::bail!("--all-pages 与 --pages 不能同时使用");
    }

    let mut selected: Vec<u32> = if all_pages {
        doc.get_pages().keys().cloned().collect()
    } else if let Some(spec) = pages {
        parse_pages(spec)?
    } else {
        vec![page]
    };

    if selected.is_empty() {
        anyhow::bail!("未选择任何页面");
    }

    let total = doc.get_pages().len() as u32;
    for p in &selected {
        if *p < 1 || *p > total {
            anyhow::bail!("页码 {} 超出范围 1~{}", p, total);
        }
    }

    selected.sort_unstable();
    selected.dedup();
    Ok(selected)
}
