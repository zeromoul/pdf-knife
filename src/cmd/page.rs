use lopdf::{Document, Object};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[allow(clippy::too_many_arguments)]
pub fn page_info(
    input: PathBuf,
    page: u32,
    set_box: Option<String>,
    rect: Vec<f32>,
    output: Option<PathBuf>,
    _show_rotate: bool,
    set_rotate: Option<i64>,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let mut doc = crate::util::load_document(&input, password)?;
    let page_id = get_page_id(&doc, page)?;

    if let Some(ref box_name) = set_box {
        if rect.len() < 4 {
            anyhow::bail!("--set-box 需要配合 --rect 提供 4 个坐标值");
        }
        let out = output
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("--set-box 需要 --output 指定输出文件"))?;
        let box_obj = Object::Array(rect.iter().copied().map(Object::Real).collect());
        doc.get_object_mut(page_id)?
            .as_dict_mut()?
            .set(box_name.as_bytes(), box_obj);
        doc.save(out)?;
        println!("✅ {} 已更新。", box_name);
        return Ok(());
    }

    if let Some(angle) = set_rotate {
        if ![0, 90, 180, 270].contains(&angle) {
            anyhow::bail!("旋转角度必须为 0/90/180/270，收到: {}", angle);
        }
        let out = output
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("--set-rotate 需要 --output 指定输出文件"))?;
        doc.get_object_mut(page_id)?
            .as_dict_mut()?
            .set("Rotate", Object::Integer(angle));
        doc.save(out)?;
        println!("✅ 第 {} 页旋转角度已设为 {}°。", page, angle);
        return Ok(());
    }

    println!("\n--- 第 {} 页尺寸信息 ---", page);
    for box_name in &["MediaBox", "CropBox", "BleedBox", "TrimBox", "ArtBox"] {
        if let Some(arr) = find_inherited_box(&doc, page_id, box_name.as_bytes()) {
            let vals: Vec<String> = arr.iter().map(fmt_box_val).collect();
            println!("  {:12}: [{}]", box_name, vals.join(", "));
        } else {
            println!("  {:12}: (未定义)", box_name);
        }
    }
    let rotate = find_inherited_i64(&doc, page_id, b"Rotate").unwrap_or(0);
    println!("  Rotate      : {}°", rotate);
    Ok(())
}

pub fn page_op(
    input: PathBuf,
    output: PathBuf,
    delete: Vec<u32>,
    rotate_specs: Vec<String>,
    keep: Vec<u32>,
    reorder: Vec<u32>,
    password: &Option<String>,
) -> anyhow::Result<()> {
    let mut doc = crate::util::load_document(&input, password)?;

    // 1. 旋转
    for spec in &rotate_specs {
        let parts: Vec<&str> = spec.splitn(2, ':').collect();
        if parts.len() != 2 {
            anyhow::bail!("--rotate 格式应为 页码:角度，如 1:90，收到: {}", spec);
        }
        let pn: u32 = parts[0]
            .parse()
            .map_err(|_| anyhow::anyhow!("无效页码: {}", parts[0]))?;
        let ang: i64 = parts[1]
            .parse()
            .map_err(|_| anyhow::anyhow!("无效角度: {}", parts[1]))?;
        if ![0i64, 90, 180, 270].contains(&ang) {
            anyhow::bail!("旋转角度须为 0/90/180/270，收到: {}", ang);
        }
        let pid = get_page_id(&doc, pn)?;
        doc.get_object_mut(pid)?
            .as_dict_mut()?
            .set("Rotate", Object::Integer(ang));
    }

    // 2. 确定要删除的页
    let all_pages: Vec<u32> = doc.get_pages().keys().cloned().collect();
    let to_delete: Vec<u32> = if !keep.is_empty() {
        all_pages
            .iter()
            .filter(|p| !keep.contains(p))
            .cloned()
            .collect()
    } else {
        delete.clone()
    };

    // 3. 从页树中移除页面（从后往前，避免索引漂移）
    let mut sorted_del = to_delete.clone();
    sorted_del.sort_unstable_by(|a, b| b.cmp(a));
    for page_num in &sorted_del {
        let pages_map = doc.get_pages();
        if let Some(&page_id) = pages_map.get(page_num) {
            let parent_id = {
                let pd = doc.get_object(page_id)?.as_dict()?;
                pd.get(b"Parent")?.as_reference()?
            };
            // 从 Kids 中移除
            {
                let parent_dict = doc.get_object_mut(parent_id)?.as_dict_mut()?;
                let kids = parent_dict.get_mut(b"Kids")?;
                if let Object::Array(ref mut arr) = *kids {
                    arr.retain(|k| k.as_reference().map(|r| r != page_id).unwrap_or(true));
                }
                let count = parent_dict
                    .get(b"Count")
                    .and_then(|c| c.as_i64())
                    .unwrap_or(1);
                parent_dict.set("Count", Object::Integer(count - 1));
            }
            // 沿祖先链向上递减 Count（多层页树）
            decrement_count_ancestors(&mut doc, parent_id);
        }
    }

    crate::util::save_document(&mut doc, &output)?;

    if !to_delete.is_empty() {
        println!("✅ 已删除页面: {:?}", to_delete);
    }
    if !rotate_specs.is_empty() {
        println!("✅ 已旋转页面: {:?}", rotate_specs);
    }
    // 4. 重排页序
    if !reorder.is_empty() {
        let pages_map = doc.get_pages();
        let total = pages_map.len() as u32;
        // 校验 reorder 列表包含了所有页码且无重复
        if reorder.len() as u32 != total {
            anyhow::bail!(
                "--reorder 需要指定所有 {} 个页码，当前只有 {}",
                total,
                reorder.len()
            );
        }
        let mut seen = std::collections::HashSet::new();
        for &p in &reorder {
            if p < 1 || p > total {
                anyhow::bail!("--reorder 页码 {} 超出范围 1~{}", p, total);
            }
            if !seen.insert(p) {
                anyhow::bail!("--reorder 页码 {} 重复", p);
            }
        }
        // 按 reorder 页码顺序构建新 Kids 数组
        let pages_map = doc.get_pages();
        let parent_id = {
            let first_page_id = *pages_map.values().next().unwrap();
            doc.get_object(first_page_id)?
                .as_dict()?
                .get(b"Parent")?
                .as_reference()?
        };
        let new_kids: Vec<Object> = reorder
            .iter()
            .map(|pnum| {
                let pid = pages_map
                    .get(pnum)
                    .ok_or_else(|| anyhow::anyhow!("--reorder: 页码 {} 不存在", pnum))?;
                Ok(Object::Reference(*pid))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let parent_dict = doc.get_object_mut(parent_id)?.as_dict_mut()?;
        parent_dict.set("Kids", Object::Array(new_kids));
        crate::util::save_document(&mut doc, &output)?;

        println!("✅ 已按新顺序重排页面: {:?}", reorder);
        return Ok(());
    }
    Ok(())
}

/// 沿 Parent 链向上（不含直接父节点，已在调用处处理）递减 Count
fn decrement_count_ancestors(doc: &mut Document, start_id: lopdf::ObjectId) {
    let mut current = start_id;
    loop {
        let parent_opt = doc
            .get_object(current)
            .and_then(|o| o.as_dict())
            .ok()
            .and_then(|d| d.get(b"Parent").and_then(|p| p.as_reference()).ok());
        let ancestor_id = match parent_opt {
            Some(id) => id,
            None => break,
        };
        if let Ok(d) = doc
            .get_object_mut(ancestor_id)
            .and_then(|o| o.as_dict_mut())
        {
            let cnt = d.get(b"Count").and_then(|c| c.as_i64()).unwrap_or(1);
            d.set("Count", Object::Integer(cnt - 1));
        }
        current = ancestor_id;
    }
}

fn fmt_box_val(v: &Object) -> String {
    match v {
        Object::Real(f) => format!("{:.2}", f),
        Object::Integer(i) => i.to_string(),
        other => format!("{:?}", other),
    }
}

/// 沿页树向上查找继承的 Box 数组
fn find_inherited_box(
    doc: &Document,
    mut node_id: lopdf::ObjectId,
    key: &[u8],
) -> Option<Vec<Object>> {
    loop {
        let dict = doc.get_object(node_id).and_then(|o| o.as_dict()).ok()?;
        if let Ok(arr) = dict.get(key).and_then(|b| b.as_array()) {
            return Some(arr.clone());
        }
        node_id = dict.get(b"Parent").and_then(|p| p.as_reference()).ok()?;
    }
}

/// 沿页树向上查找继承的 i64 属性
fn find_inherited_i64(doc: &Document, mut node_id: lopdf::ObjectId, key: &[u8]) -> Option<i64> {
    loop {
        let dict = doc.get_object(node_id).and_then(|o| o.as_dict()).ok()?;
        if let Ok(v) = dict.get(key).and_then(|v| v.as_i64()) {
            return Some(v);
        }
        node_id = dict.get(b"Parent").and_then(|p| p.as_reference()).ok()?;
    }
}

fn get_page_id(doc: &Document, page: u32) -> anyhow::Result<lopdf::ObjectId> {
    doc.get_pages()
        .get(&page)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("页码 {} 超出范围", page))
}
/// 合并多个 PDF（安全方式：重新分配对象ID避免冲突）
pub fn merge(
    inputs: Vec<PathBuf>,
    pages: Vec<String>,
    output: PathBuf,
    password: &Option<String>,
) -> anyhow::Result<()> {
    if inputs.is_empty() {
        anyhow::bail!("merge 至少需要一个输入文件");
    }
    if inputs.len() == 1 && pages.is_empty() {
        // 只有一个文件，直接复制
        std::fs::copy(&inputs[0], &output)?;
        println!("✅ 已复制 {:?} -> {:?}", inputs[0], output);
        return Ok(());
    }

    if !pages.is_empty() && pages.len() != 1 && pages.len() != inputs.len() {
        anyhow::bail!(
            "--pages 参数数量不正确：应为 1 或与输入文件数一致（{}），当前为 {}",
            inputs.len(),
            pages.len()
        );
    }

    let mut max_id = 1;
    let mut documents_pages: BTreeMap<lopdf::ObjectId, Object> = BTreeMap::new();
    let mut documents_objects: BTreeMap<lopdf::ObjectId, Object> = BTreeMap::new();
    let mut merged_doc = Document::with_version("1.5");

    for (idx, path) in inputs.iter().enumerate() {
        let mut doc = crate::util::load_document(path, password)?;
        let pages_map = doc.get_pages();
        let total_pages = pages_map.len() as u32;

        let selected_page_nums: Vec<u32> = if pages.is_empty() {
            pages_map.keys().cloned().collect()
        } else {
            let spec = if pages.len() == 1 {
                &pages[0]
            } else {
                &pages[idx]
            };
            let selected = crate::util::parse_pages(spec)?;
            if selected.is_empty() {
                anyhow::bail!(
                    "文件 {:?} 的页码范围为空：{}",
                    path.file_name().unwrap_or_default(),
                    spec
                );
            }
            for p in &selected {
                if *p < 1 || *p > total_pages {
                    anyhow::bail!(
                        "文件 {:?} 的页码 {} 超出范围 1~{}",
                        path.file_name().unwrap_or_default(),
                        p,
                        total_pages
                    );
                }
            }
            selected
        };
        let selected_count = selected_page_nums.len();

        // 按官方示例重排对象 ID，避免跨文档冲突
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;

        let pages = doc.get_pages();
        for page_num in selected_page_nums {
            let object_id = *pages
                .get(&page_num)
                .ok_or_else(|| anyhow::anyhow!("页码 {} 不存在", page_num))?;
            let object = doc.get_object(object_id)?.to_owned();
            documents_pages.insert(object_id, object);
        }

        documents_objects.extend(doc.objects);

        println!(
            "  [{}] 已读取 {:?} (选中 {}/{} 页)",
            idx + 1,
            path.file_name().unwrap_or_default(),
            selected_count,
            total_pages
        );
    }

    let mut catalog_object: Option<(lopdf::ObjectId, Object)> = None;
    let mut pages_object: Option<(lopdf::ObjectId, Object)> = None;

    // 先合并非 Page 对象，并选定 Catalog/Pages 根对象
    for (object_id, object) in documents_objects {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                catalog_object = Some((
                    if let Some((id, _)) = catalog_object {
                        id
                    } else {
                        object_id
                    },
                    object,
                ));
            }
            b"Pages" => {
                if let Ok(dictionary) = object.as_dict() {
                    let mut dictionary = dictionary.clone();
                    if let Some((_, ref old_object)) = pages_object
                        && let Ok(old_dictionary) = old_object.as_dict()
                    {
                        dictionary.extend(old_dictionary);
                    }
                    pages_object = Some((
                        if let Some((id, _)) = pages_object {
                            id
                        } else {
                            object_id
                        },
                        Object::Dictionary(dictionary),
                    ));
                }
            }
            b"Page" | b"Outlines" | b"Outline" => {}
            _ => {
                merged_doc.objects.insert(object_id, object);
            }
        }
    }

    let (pages_id, pages_obj) =
        pages_object.ok_or_else(|| anyhow::anyhow!("合并失败：未找到 Pages 根对象"))?;
    let (catalog_id, catalog_obj) =
        catalog_object.ok_or_else(|| anyhow::anyhow!("合并失败：未找到 Catalog 根对象"))?;

    // 写入所有 Page，并统一 Parent 指向新的 Pages 根
    for (object_id, object) in &documents_pages {
        if let Ok(dictionary) = object.as_dict() {
            let mut dictionary = dictionary.clone();
            dictionary.set("Parent", pages_id);
            merged_doc
                .objects
                .insert(*object_id, Object::Dictionary(dictionary));
        }
    }

    // 重建 Pages
    if let Ok(dictionary) = pages_obj.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Count", documents_pages.len() as u32);
        dictionary.set(
            "Kids",
            documents_pages
                .keys()
                .map(|id| Object::Reference(*id))
                .collect::<Vec<_>>(),
        );
        merged_doc
            .objects
            .insert(pages_id, Object::Dictionary(dictionary));
    }

    // 重建 Catalog
    if let Ok(dictionary) = catalog_obj.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Pages", pages_id);
        dictionary.remove(b"Outlines");
        merged_doc
            .objects
            .insert(catalog_id, Object::Dictionary(dictionary));
    }

    merged_doc.trailer.set("Root", catalog_id);
    merged_doc.max_id = merged_doc
        .objects
        .keys()
        .map(|(id, _)| *id)
        .max()
        .unwrap_or(1);
    merged_doc.renumber_objects();
    crate::util::save_document(&mut merged_doc, &output)?;

    println!("✅ 合并完成，输出: {:?}", output);
    Ok(())
}
