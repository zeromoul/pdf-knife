use crate::util::resolve_resources;
use lopdf::Document;
use std::path::PathBuf;

#[allow(clippy::too_many_arguments)]
pub fn list_res(
    input: PathBuf,
    page: u32,
    pages: Option<String>,
    all_pages: bool,
    query: Option<String>,
    name: Option<String>,
    show_stream: bool,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;
    let pages_to_run = crate::util::select_pages(&doc, page, &pages, all_pages)?;
    let multi = pages_to_run.len() > 1;
    for cur_page in &pages_to_run {
        if multi {
            println!("\n====== 第 {} 页 ======", cur_page);
        }
        let page_id = get_page_id(&doc, *cur_page)?;
        let resources = resolve_resources(&doc, page_id)?;

        if let Some(ref target_name) = name
            && show_stream
        {
            let mut found = false;
            'outer: for (_, val) in &resources {
                let sub = resolve_sub_dict(&doc, val);
                if let Ok(sub) = sub {
                    for (k, v) in &sub {
                        if String::from_utf8_lossy(k) == target_name.as_str() {
                            let id = v.as_reference()?;
                            let stream = doc.get_object(id)?.as_stream()?;
                            let decoded = stream.decompressed_content()?;
                            println!("--- /{} 内容流 (ID: {:?}) ---", target_name, id);
                            println!("{}", String::from_utf8_lossy(&decoded));
                            found = true;
                            break 'outer;
                        }
                    }
                }
            }
            if !found {
                println!("❌ 未找到资源: /{}", target_name);
            }
            continue;
        }

        for (type_key, val) in &resources {
            let type_name = String::from_utf8_lossy(type_key);
            if let Ok(sub) = resolve_sub_dict(&doc, val) {
                for (res_key, res_val) in &sub {
                    let res_name_str = String::from_utf8_lossy(res_key);
                    if let Some(ref q) = query
                        && !res_name_str.contains(q)
                    {
                        continue;
                    }
                    if let Some(ref target) = name
                        && res_name_str.as_ref() != target.as_str()
                    {
                        continue;
                    }
                    print!("  - /{} (类型: {})", res_name_str, type_name);
                    if let Ok(id) = res_val.as_reference() {
                        print!(" [ID: {:?}]", id);
                        if let Ok(stream) = doc.get_object(id)?.as_stream() {
                            if let Ok(sub) = stream.dict.get(b"Subtype").and_then(|s| s.as_name()) {
                                print!(" Subtype=/{}", String::from_utf8_lossy(sub));
                            }
                            print!(" 流大小={} 字节", stream.content.len());
                        }
                    }
                    println!();
                }
            }
        }
    }
    Ok(())
}

pub fn list_images(
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
        if multi {
            println!("\n====== 第 {} 页 ======", cur_page);
        }
        let page_id = get_page_id(&doc, *cur_page)?;
        let resources = resolve_resources(&doc, page_id)?;

        if let Ok(xobjs) = resources.get(b"XObject").and_then(|x| x.as_dict()) {
            for (name, val) in xobjs {
                if let Ok(id) = val.as_reference()
                    && let Ok(stream) = doc.get_object(id)?.as_stream()
                    && stream
                        .dict
                        .get(b"Subtype")
                        .is_ok_and(|s| s.as_name().is_ok_and(|n| n == b"Image"))
                {
                    let w = stream
                        .dict
                        .get(b"Width")
                        .map(|v| format!("{:?}", v))
                        .unwrap_or("?".into());
                    let h = stream
                        .dict
                        .get(b"Height")
                        .map(|v| format!("{:?}", v))
                        .unwrap_or("?".into());
                    let filter = stream
                        .dict
                        .get(b"Filter")
                        .and_then(|f| f.as_name())
                        .map(|n| String::from_utf8_lossy(n).to_string())
                        .unwrap_or("无".into());
                    let cs = stream
                        .dict
                        .get(b"ColorSpace")
                        .and_then(|c| c.as_name())
                        .map(|n| String::from_utf8_lossy(n).to_string())
                        .unwrap_or("?".into());
                    println!(
                        "/{} [ID:{:?}] {}x{} Filter={} ColorSpace={} 大小={}B",
                        String::from_utf8_lossy(name),
                        id,
                        w,
                        h,
                        filter,
                        cs,
                        stream.content.len()
                    );
                }
            }
        } else {
            println!("该页没有 XObject 图片。");
        }
    }
    Ok(())
}

pub fn extract_image(
    input: PathBuf,
    page: u32,
    res_name: String,
    output: PathBuf,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let doc = crate::util::load_document(&input, password)?;
    let page_id = get_page_id(&doc, page)?;
    let resources = resolve_resources(&doc, page_id)?;
    let xobjs = resources.get(b"XObject")?.as_dict()?;
    let id = xobjs.get(res_name.as_bytes())?.as_reference()?;
    let stream = doc.get_object(id)?.as_stream()?;
    let filter = stream
        .dict
        .get(b"Filter")
        .and_then(|f| f.as_name())
        .map(|n| String::from_utf8_lossy(n).to_string())
        .unwrap_or_default();
    let cs = stream
        .dict
        .get(b"ColorSpace")
        .and_then(|c| c.as_name())
        .map(|n| String::from_utf8_lossy(n).to_string())
        .unwrap_or_default();
    let data = if filter == "DCTDecode" {
        stream.content.clone()
    } else {
        stream.decompressed_content()?
    };
    std::fs::write(&output, &data)?;
    println!(
        "✅ 图片已提取 (Filter:{}, ColorSpace:{}, {}B) -> {:?}",
        if filter.is_empty() { "无" } else { &filter },
        if cs.is_empty() { "?" } else { &cs },
        data.len(),
        output
    );
    Ok(())
}

pub fn del_res(
    input: PathBuf,
    output: PathBuf,
    page: u32,
    res_type: String,
    res_name: String,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let mut doc = crate::util::load_document(&input, password)?;
    let page_id = get_page_id(&doc, page)?;
    let res = doc
        .get_object_mut(page_id)?
        .as_dict_mut()?
        .get_mut(b"Resources")?
        .as_dict_mut()?;
    res.get_mut(res_type.as_bytes())?
        .as_dict_mut()?
        .remove(res_name.as_bytes());
    crate::util::save_document(&mut doc, output)?;
    println!("✅ 资源 /{} 已从 {} 中移除。", res_name, res_type);
    Ok(())
}

fn resolve_sub_dict(doc: &Document, val: &lopdf::Object) -> anyhow::Result<lopdf::Dictionary> {
    Ok(if let Ok(id) = val.as_reference() {
        doc.get_object(id)?.as_dict()?.clone()
    } else {
        val.as_dict()?.clone()
    })
}

fn get_page_id(doc: &Document, page: u32) -> anyhow::Result<lopdf::ObjectId> {
    doc.get_pages()
        .get(&page)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("页码 {} 超出范围", page))
}
