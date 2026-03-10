use lopdf::Object;
use std::path::PathBuf;

pub fn obj(
    input: PathBuf,
    id: Option<u32>,
    show_trailer: bool,
    show_catalog: bool,
    show_stream: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;

    if show_trailer {
        println!("--- Trailer 字典 ---");
        for (k, v) in &doc.trailer {
            println!("  /{:<16}: {:?}", String::from_utf8_lossy(k), v);
        }
        return Ok(());
    }

    if show_catalog {
        let root_id = doc.trailer.get(b"Root")?.as_reference()?;
        let root = doc.get_object(root_id)?.as_dict()?;
        println!("--- Catalog Root ({} {} R) ---", root_id.0, root_id.1);
        for (k, v) in root {
            println!("  /{:<16}: {:?}", String::from_utf8_lossy(k), v);
        }
        return Ok(());
    }

    if let Some(obj_num) = id {
        let mut found = false;
        for (&(num, g), obj) in &doc.objects {
            if num == obj_num {
                found = true;
                println!("--- Object {} {} R ---", num, g);
                match obj {
                    Object::Stream(s) => {
                        println!("  [Stream] dict:");
                        for (k, v) in &s.dict {
                            println!("    /{:<16}: {:?}", String::from_utf8_lossy(k), v);
                        }
                        if show_stream {
                            let decoded = s.decompressed_content()?;
                            println!("\n[--stream content ({} bytes)--]", decoded.len());
                            println!("{}", String::from_utf8_lossy(&decoded));
                        }
                    }
                    Object::Dictionary(d) => {
                        println!("  [Dictionary]");
                        for (k, v) in d {
                            println!("    /{:<16}: {:?}", String::from_utf8_lossy(k), v);
                        }
                    }
                    other => println!("  {:?}", other),
                }
                break;
            }
        }
        if !found {
            println!("未找到对象 ID = {}", obj_num);
        }
        return Ok(());
    }

    // 列出所有对象
    println!("--- 文档对象列表 (共 {} 个) ---", doc.objects.len());
    let mut sorted: Vec<_> = doc.objects.iter().collect();
    sorted.sort_by_key(|(id, _)| *id);
    for (&(num, g), obj) in sorted.iter().copied() {
        let hint = type_hint(obj);
        println!("{:6} {:3} R  {}", num, g, hint);
    }
    Ok(())
}

fn type_hint(obj: &Object) -> String {
    match obj {
        Object::Dictionary(d) => {
            let t = d
                .get(b"Type")
                .ok()
                .and_then(|v| v.as_name().ok())
                .map(|n| format!("/{}", String::from_utf8_lossy(n)))
                .unwrap_or_default();
            let s = d
                .get(b"Subtype")
                .ok()
                .and_then(|v| v.as_name().ok())
                .map(|n| format!("/{}", String::from_utf8_lossy(n)))
                .unwrap_or_default();
            format!("Dict{}{}", t, s)
        }
        Object::Stream(s) => {
            let t = s
                .dict
                .get(b"Type")
                .ok()
                .and_then(|v| v.as_name().ok())
                .map(|n| format!("/{}", String::from_utf8_lossy(n)))
                .unwrap_or_default();
            let sub = s
                .dict
                .get(b"Subtype")
                .ok()
                .and_then(|v| v.as_name().ok())
                .map(|n| format!("/{}", String::from_utf8_lossy(n)))
                .unwrap_or_default();
            format!("Stream{}{} ({}B raw)", t, sub, s.content.len())
        }
        Object::Array(a) => format!("Array[{}]", a.len()),
        Object::Integer(n) => format!("Integer({})", n),
        Object::Real(f) => format!("Real({})", f),
        Object::String(s, _) => {
            let decoded = lopdf::decode_text_string(obj)
                .unwrap_or_else(|_| String::from_utf8_lossy(&s[..s.len().min(24)]).to_string());
            format!(
                "String({:?}...)",
                decoded.chars().take(24).collect::<String>()
            )
        }
        Object::Name(n) => format!("Name({})", String::from_utf8_lossy(n)),
        Object::Boolean(b) => format!("Boolean({})", b),
        Object::Null => "Null".to_string(),
        Object::Reference(r) => format!("Ref({} {})", r.0, r.1),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn font_info(
    input: PathBuf,
    page: u32,
    pages: Option<String>,
    all_pages: bool,
    name: Option<String>,
    cmap: bool,
    widths: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;
    let pages_to_run = crate::util::select_pages(&doc, page, &pages, all_pages)?;
    let multi = pages_to_run.len() > 1;
    for cur_page in &pages_to_run {
        if multi {
            println!("\n====== 第 {} 页 ======", cur_page);
        }
        let page_map = doc.get_pages();
        let page_id = *page_map
            .get(cur_page)
            .ok_or_else(|| anyhow::anyhow!("页码 {} 超出范围", cur_page))?;

        let resources = crate::util::resolve_resources(&doc, page_id)?;

        let font_dict = match resources.get(b"Font") {
            Ok(f) => {
                if let Ok(r) = f.as_reference() {
                    doc.get_object(r)?.as_dict()?.clone()
                } else {
                    f.as_dict()?.clone()
                }
            }
            Err(_) => {
                println!("该页面没有字体资源。");
                continue;
            }
        };

        let mut printed = 0usize;
        for (font_key, font_ref) in &font_dict {
            let fname = String::from_utf8_lossy(font_key).to_string();
            if let Some(ref target) = name
                && &fname != target
            {
                continue;
            }
            let fid = match font_ref.as_reference() {
                Ok(id) => id,
                Err(_) => continue,
            };
            let font_obj = doc.get_object(fid)?.as_dict()?;
            println!("\n--- 字体 /{} ({} {} R) ---", fname, fid.0, fid.1);

            macro_rules! name_key {
                ($k:expr) => {
                    font_obj
                        .get($k)
                        .ok()
                        .and_then(|v| v.as_name().ok())
                        .map(|n| format!("/{}", String::from_utf8_lossy(n)))
                        .unwrap_or_else(|| "(未设置)".to_string())
                };
            }
            println!("  Subtype  : {}", name_key!(b"Subtype"));
            println!("  BaseFont : {}", name_key!(b"BaseFont"));

            // Encoding
            if let Ok(enc) = font_obj.get(b"Encoding") {
                match enc {
                    Object::Name(n) => println!("  Encoding : /{}", String::from_utf8_lossy(n)),
                    Object::Reference(r) => {
                        let enc_dict = doc.get_object(*r)?.as_dict()?;
                        let base = enc_dict
                            .get(b"BaseEncoding")
                            .ok()
                            .and_then(|v| v.as_name().ok())
                            .map(|n| format!("/{}", String::from_utf8_lossy(n)))
                            .unwrap_or_else(|| "(无 BaseEncoding)".to_string());
                        println!(
                            "  Encoding : EncodingDict [{} {} R] BaseEncoding={}",
                            r.0, r.1, base
                        );
                        if let Ok(diffs) = enc_dict.get(b"Differences").and_then(|d| d.as_array()) {
                            let s: Vec<String> =
                                diffs.iter().take(32).map(|v| format!("{:?}", v)).collect();
                            println!("  Differences (前32): {}", s.join(" "));
                        }
                    }
                    _ => println!("  Encoding : {:?}", enc),
                }
            } else {
                println!("  Encoding : (未设置)");
            }

            // Type0 复合字体 — DescendantFonts[0] 包含实际子字体信息
            let subtype_is_type0 = font_obj
                .get(b"Subtype")
                .ok()
                .and_then(|v| v.as_name().ok())
                .map(|n| n == b"Type0")
                .unwrap_or(false);
            if subtype_is_type0
                && let Ok(desc_arr) = font_obj.get(b"DescendantFonts").and_then(|v| v.as_array())
                && let Some(desc_ref) = desc_arr.first()
            {
                let desc_id = desc_ref.as_reference()?;
                let desc = doc.get_object(desc_id)?.as_dict()?;
                let sub2 = desc
                    .get(b"Subtype")
                    .ok()
                    .and_then(|v| v.as_name().ok())
                    .map(|n| format!("/{}", String::from_utf8_lossy(n)))
                    .unwrap_or_else(|| "?".into());
                let base2 = desc
                    .get(b"BaseFont")
                    .ok()
                    .and_then(|v| v.as_name().ok())
                    .map(|n| format!("/{}", String::from_utf8_lossy(n)))
                    .unwrap_or_else(|| "?".into());
                println!("  DescendantFont ({} {} R):", desc_id.0, desc_id.1);
                println!("    Subtype  : {}", sub2);
                println!("    BaseFont : {}", base2);
                if let Ok(cid_info) = desc.get(b"CIDSystemInfo").and_then(|v| v.as_dict()) {
                    let registry = cid_info
                        .get(b"Registry")
                        .and_then(|v| v.as_str())
                        .map(|b| String::from_utf8_lossy(b).to_string())
                        .unwrap_or_default();
                    let ordering = cid_info
                        .get(b"Ordering")
                        .and_then(|v| v.as_str())
                        .map(|b| String::from_utf8_lossy(b).to_string())
                        .unwrap_or_default();
                    println!("    CIDSystem: {}-{}", registry, ordering);
                }
            }

            // ToUnicode
            if let Ok(to_u_ref) = font_obj.get(b"ToUnicode").and_then(|v| v.as_reference()) {
                println!("  ToUnicode: ✅ ({} {} R)", to_u_ref.0, to_u_ref.1);
                if cmap && let Ok(s) = doc.get_object(to_u_ref)?.as_stream() {
                    let data = s.decompressed_content()?;
                    println!("  [ToUnicode CMap]");
                    println!("{}", String::from_utf8_lossy(&data));
                }
            } else {
                println!("  ToUnicode: ❌ (无 — 可能出现乱码)");
            }

            // 字符范围（简单字体）
            if let Ok(fc) = font_obj.get(b"FirstChar").and_then(|v| v.as_i64())
                && let Ok(lc) = font_obj.get(b"LastChar").and_then(|v| v.as_i64())
            {
                println!("  CharRange: {} ~ {} ({} 个字符)", fc, lc, lc - fc + 1);
                // 宽度表
                if widths {
                    if let Ok(w_arr) = font_obj.get(b"Widths").and_then(|v| v.as_array()) {
                        println!("  Widths ({} 项):", w_arr.len());
                        let cols = 16usize;
                        for (chunk_i, chunk) in w_arr.chunks(cols).enumerate() {
                            let start_char = fc as usize + chunk_i * cols;
                            let vals: Vec<String> = chunk
                                .iter()
                                .map(|w| {
                                    let v = w.as_f32().unwrap_or(0.0);
                                    format!("{:5.0}", v)
                                })
                                .collect();
                            println!("    [{}..]: {}", start_char, vals.join(" "));
                        }
                    } else if subtype_is_type0 {
                        println!("  Widths: Type0 复合字体不使用 Widths（见 DW/W 字段）");
                    }
                }
            }
            printed += 1;
        }
        if printed == 0
            && let Some(ref n) = name
        {
            println!("未找到字体 /{}", n);
        }
    } // end for cur_page
    Ok(())
}

/// 直接修改某对象字典中的一个键值并保存
pub fn set_obj(
    input: PathBuf,
    output: PathBuf,
    id: u32,
    key: String,
    value: String,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let mut doc = crate::util::load_document(&input, password)?;

    // 按对象号查找 (代/代号+第几代 任意)
    let obj_id = doc
        .objects
        .keys()
        .find(|(n, _)| *n == id)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("未找到对象编号 = {}", id))?;

    if value == "delete" {
        let obj = doc.get_object_mut(obj_id)?;
        let dict = match obj {
            Object::Dictionary(d) => d,
            Object::Stream(s) => &mut s.dict,
            _ => anyhow::bail!("对象 {} {} R 不是字典/流，无法删除键", obj_id.0, obj_id.1),
        };
        dict.remove(key.as_bytes());
        println!("✅ 已删除 {} {} R /{}", obj_id.0, obj_id.1, key);
    } else {
        let new_val = parse_pdf_value(&value)?;
        let obj = doc.get_object_mut(obj_id)?;
        let dict = match obj {
            Object::Dictionary(d) => d,
            Object::Stream(s) => &mut s.dict,
            _ => anyhow::bail!("对象 {} {} R 不是字典/流，无法设置键", obj_id.0, obj_id.1),
        };
        dict.set(key.as_bytes(), new_val);
        println!("✅ 已设置 {} {} R /{} = {}", obj_id.0, obj_id.1, key, value);
    }

    crate::util::save_document(&mut doc, &output)?;
    Ok(())
}

fn parse_pdf_value(s: &str) -> anyhow::Result<Object> {
    if s == "null" {
        return Ok(Object::Null);
    }
    if s == "true" {
        return Ok(Object::Boolean(true));
    }
    if s == "false" {
        return Ok(Object::Boolean(false));
    }
    if let Some(name) = s.strip_prefix('/') {
        return Ok(Object::Name(name.as_bytes().to_vec()));
    }
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        return Ok(Object::String(
            inner.as_bytes().to_vec(),
            lopdf::StringFormat::Literal,
        ));
    }
    if let Ok(i) = s.parse::<i64>() {
        return Ok(Object::Integer(i));
    }
    if let Ok(f) = s.parse::<f64>() {
        return Ok(Object::Real(f as f32));
    }
    anyhow::bail!(
        "无法解析值 {:?}（支持: 整数/实数/true/false/null//Name/\"string\"/delete）",
        s
    )
}
