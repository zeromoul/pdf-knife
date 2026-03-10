use std::path::PathBuf;

pub fn list_annots(
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
        let page_id = doc
            .get_pages()
            .get(cur_page)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("页码 {} 超出范围", cur_page))?;

        if let Ok(annots) = doc
            .get_object(page_id)?
            .as_dict()?
            .get(b"Annots")
            .and_then(|o| o.as_array())
        {
            println!("\n--- 第 {} 页 注释 (共 {} 个) ---", cur_page, annots.len());
            for (i, obj) in annots.iter().enumerate() {
                if let Ok(id) = obj.as_reference()
                    && let Ok(d) = doc.get_object(id)?.as_dict()
                {
                    let sub = d.get(b"Subtype").and_then(|s| s.as_name()).unwrap_or(b"?");
                    let rect = d
                        .get(b"Rect")
                        .and_then(|r| r.as_array())
                        .map(|a| format!("{:?}", a))
                        .unwrap_or("[]".into());
                    println!("\n[注释 #{}] ID: {:?}", i + 1, id);
                    println!("  类型: /{}", String::from_utf8_lossy(sub));
                    println!("  位置: {}", rect);
                    if let Ok(c) = d.get(b"Contents") {
                        let decoded_ct = lopdf::decode_text_string(c).unwrap_or_else(|_| {
                            String::from_utf8_lossy(c.as_str().unwrap_or(b"")).to_string()
                        });
                        println!("  内容: {}", decoded_ct);
                    }
                    if let Ok(action) = d.get(b"A").and_then(|a| a.as_dict()) {
                        if let Ok(uri_obj) = action.get(b"URI") {
                            let decoded_uri =
                                lopdf::decode_text_string(uri_obj).unwrap_or_else(|_| {
                                    String::from_utf8_lossy(uri_obj.as_str().unwrap_or(b""))
                                        .to_string()
                                });
                            println!("  URI: {}", decoded_uri);
                        }
                        if let Ok(s) = action.get(b"S").and_then(|s| s.as_name()) {
                            println!("  Action: /{}", String::from_utf8_lossy(s));
                        }
                    }
                }
            }
        } else if multi {
            println!("第 {} 页没有注释。", cur_page);
        } else {
            println!("✅ 第 {} 页没有注释。", cur_page);
        }
    }
    Ok(())
}
