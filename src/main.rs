mod cmd;
mod util;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "pdf-knife", author = "molin", version = "0.1.0",about = "PDF 内容流和资源分析修改工具", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// 文档打开密码 (针对加密 PDF)
    #[arg(long, global = true)]
    password: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// 提取指定页面的内容流 (Contents) 到文件
    Extract {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 页码范围，如 "1,3,5-8"（与 --all-pages 互斥）
        #[arg(long)]
        pages: Option<String>,
        /// 处理所有页面
        #[arg(long)]
        all_pages: bool,
        /// 输出文件路径（不传则自动使用默认命名）
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// 从文本文件导入内容流替换指定页面的原有内容
    Import {
        /// 输入 PDF 文件路径
        input_pdf: PathBuf,
        /// 输出 PDF 文件路径
        output_pdf: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 内容流文本文件路径
        #[arg(short, long)]
        stream_file: PathBuf,
        /// 导入前去除多余空白字符
        #[arg(long)]
        strip_whitespace: bool,
    },
    /// 列出页面的资源对象
    ListRes {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 页码范围，如 "1,3,5-8"（与 --all-pages 互斥）
        #[arg(long)]
        pages: Option<String>,
        /// 处理所有页面
        #[arg(long)]
        all_pages: bool,
        /// 按资源名或类型进行关键词过滤
        #[arg(long)]
        query: Option<String>,
        /// 精确匹配资源名（如 Im0、F1）
        #[arg(long)]
        name: Option<String>,
        /// 同时显示资源流内容摘要
        #[arg(long)]
        show_stream: bool,
    },
    /// 查看页面文本信息及其定位 (X, Y 坐标)
    TextInfo {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 处理所有页面
        #[arg(long)]
        all_pages: bool,
    },
    /// 列出/过滤页面操作符
    ListOps {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 按操作符分类过滤（如 textshow/path/color）
        #[arg(short, long)]
        category: Option<String>,
        /// 指定操作符过滤（可传多个，如 --op Tj TJ）
        #[arg(long, num_args = 0..)]
        op: Vec<String>,
        /// 输出各操作符统计信息
        #[arg(long)]
        stats: bool,
        /// 输出操作符在流内字节偏移
        #[arg(long)]
        offsets: bool,
        /// 解析指定 Form XObject（资源名，如 Fm0）的内部操作符
        #[arg(long)]
        xobject: Option<String>,
        /// 处理所有页面
        #[arg(long)]
        all_pages: bool,
    },
    /// 根据序号删除页面操作符
    DeleteOps {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 输出 PDF 文件路径
        output: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 删除范围（起止序号），如 --range 10 20
        #[arg(long, num_args = 2)]
        range: Vec<usize>,
        /// 需要跳过的操作符序号（可多个）
        #[arg(long, num_args = 0..)]
        skip: Vec<usize>,
    },
    /// 在内容流中搜索并替换字符串
    Replace {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 输出 PDF 文件路径
        output: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 旧字符串（必填）
        #[arg(long)]
        old: String,
        /// 新字符串（默认空串，表示删除）
        #[arg(long, default_value = "")]
        new: String,
        /// 启用正则替换模式
        #[arg(short, long)]
        regex: bool,
    },
    /// 查看图片详情
    ListImages {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 页码范围，如 "1,3,5-8"（与 --all-pages 互斥）
        #[arg(long)]
        pages: Option<String>,
        /// 处理所有页面
        #[arg(long)]
        all_pages: bool,
    },

    /// 提取资源图片
    ExtractImage {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 资源名（如 Im0）
        #[arg(long)]
        res_name: String,
        /// 输出图片文件路径
        #[arg(short, long)]
        output: PathBuf,
    },
    /// 输出内容流的十六进制字节视图
    HexView {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 页码范围，如 "1,3,5-8"（与 --all-pages 互斥）
        #[arg(long)]
        pages: Option<String>,
        /// 处理所有页面
        #[arg(long)]
        all_pages: bool,
    },
    /// 列出所有注释
    ListAnnots {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 页码范围，如 "1,3,5-8"（与 --all-pages 互斥）
        #[arg(long)]
        pages: Option<String>,
        /// 处理所有页面
        #[arg(long)]
        all_pages: bool,
    },
    /// 对页面进行深度检查，输出文本、位置、注释和资源问题的综合报告
    Inspect {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 页码范围，如 "1,3,5-8"（与 --all-pages 互斥）
        #[arg(long)]
        pages: Option<String>,
        /// 处理所有页面
        #[arg(long)]
        all_pages: bool,
        /// 启用注释检查
        #[arg(long)]
        annots: bool,
        /// 启用资源检查
        #[arg(long)]
        resources: bool,
        /// 输出原始调试信息
        #[arg(long)]
        raw: bool,
        /// 启用文本检查
        #[arg(long)]
        text: bool,
    },
    /// 移除资源
    DelRes {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 输出 PDF 文件路径
        output: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 资源类型（如 Font / XObject / ExtGState）
        #[arg(long)]
        res_type: String,
        /// 资源名（如 F1 / Im0）
        #[arg(long)]
        res_name: String,
    },
    /// 查看任意对象的原始内容 (对象树浏览)
    Obj {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 指定对象 ID
        #[arg(long)]
        id: Option<u32>,
        /// 输出 Trailer 字典
        #[arg(long)]
        trailer: bool,
        /// 输出 Catalog 对象
        #[arg(long)]
        catalog: bool,
        /// 同时显示对象流内容
        #[arg(long)]
        stream: bool,
    },
    /// 查看字体编码/ToUnicode 信息
    FontInfo {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 页码范围，如 "1,3,5-8"（与 --all-pages 互斥）
        #[arg(long)]
        pages: Option<String>,
        /// 处理所有页面
        #[arg(long)]
        all_pages: bool,
        /// 指定字体资源名（如 F1）
        #[arg(long)]
        name: Option<String>,
        /// 显示 ToUnicode CMap 摘要
        #[arg(long)]
        cmap: bool,
        /// 显示 Widths 宽度表
        #[arg(long)]
        widths: bool,
    },
    /// 查看页面所有裁剪框尺寸及旋转
    PageInfo {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 设置某个页面盒（MediaBox/CropBox/BleedBox/TrimBox/ArtBox）
        #[arg(long)]
        set_box: Option<String>,
        /// 盒子坐标：llx lly urx ury（共 4 个数字）
        #[arg(long, num_args = 4, allow_hyphen_values = true)]
        rect: Vec<f32>,
        /// 输出 PDF 文件路径（当执行修改时必填）
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// 显示旋转角信息
        #[arg(long)]
        rotate: bool,
        /// 设置旋转角度（0/90/180/270）
        #[arg(long)]
        set_rotate: Option<i64>,
    },
    /// 查看/导航书签大纲
    Outline {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 最大展开层级（不传为全部）
        #[arg(long)]
        depth: Option<usize>,
    },
    /// 页面操作 (删除/旋转/保留子集/重排页序)
    PageOp {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 输出 PDF 文件路径
        output: PathBuf,
        /// 删除页码列表（可多个）
        #[arg(long, num_args = 0..)]
        delete: Vec<u32>,
        /// 旋转规则（格式：页码:角度，如 1:90）
        #[arg(long, num_args = 0..)]
        rotate: Vec<String>,
        /// 仅保留这些页码（可多个）
        #[arg(long, num_args = 0..)]
        keep: Vec<u32>,
        /// 重排页序，逐一列出新顺序页码（如 --reorder 3 1 2）
        #[arg(long, num_args = 0..)]
        reorder: Vec<u32>,
    },
    /// 查看文档基本信息 (版本/加密/元数据/权限)
    DocInfo {
        /// 输入 PDF 文件路径
        input: PathBuf,
    },
    /// 输出页面结构树形分析报告（文字/图片/注释/表单/水印提示）
    Analyze {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 页码（从 1 开始，默认第 1 页）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 页码范围，如 "1,3,5-8"
        #[arg(long)]
        pages: Option<String>,
        /// 分析所有页面
        #[arg(long)]
        all_pages: bool,
    },
    /// 合并多个 PDF 为一个
    #[command(
        after_long_help = "示例:\n  1) 合并全部页面\n     pdf-surgeon merge a.pdf b.pdf -o out.pdf\n\n  2) 所有输入文件都只取同一页范围\n     pdf-surgeon merge a.pdf b.pdf --pages 1,3,5-8 -o out.pdf\n\n  3) 每个输入文件使用各自页范围（数量需与输入文件一致）\n     pdf-surgeon merge a.pdf b.pdf c.pdf --pages 1-2 3 5-7 -o out.pdf"
    )]
    Merge {
        /// 输入文件列表 (按顺序合并)
        #[arg(num_args = 1..)]
        inputs: Vec<PathBuf>,
        /// 指定要合并的页码范围（如: "1,3,5-8"）
        ///
        /// 规则：
        /// 1) 不传：每个输入文件合并全部页面
        /// 2) 传 1 个：对所有输入文件使用同一范围
        /// 3) 传 N 个：N 必须等于输入文件数，按顺序一一对应
        #[arg(long, num_args = 1.., verbatim_doc_comment)]
        pages: Vec<String>,
        /// 输出 PDF 文件路径
        #[arg(short, long)]
        output: PathBuf,
    },
    /// 直接修改任意对象的字典键值
    SetObj {
        /// 输入 PDF 文件路径
        input: PathBuf,
        /// 输出 PDF 文件路径
        output: PathBuf,
        /// 对象 ID 编号
        #[arg(long)]
        id: u32,
        /// 要设置的键名 (不含 /)
        #[arg(long)]
        key: String,
        /// 新值内容 (支持: 整数 / 实数 / 布尔 true|false / 名字 /Name / 字符串 "..." / null / 删除 delete)
        #[arg(long)]
        value: String,
    },
    /// 修改内容流中指定序号操作符的操作数（改文本/颜色/位置等）
    ///
    /// 操作数格式：数字直接写 0.5 / 名字加斜杠 /F1 / 文本直接写 Hello
    /// 示例（改颜色）: patch-op -p 1 --index 42 --operator rg --operands 1.0 0 0 -i a.pdf -o b.pdf
    /// 示例（改位置）: patch-op -p 1 --index 10 --operator Td --operands 100 200 -i a.pdf -o b.pdf
    #[command(
        after_long_help = "操作数规则:\n  数字   → 直接写 1  0.5  -10\n  名字   → /F1  /DeviceRGB\n  文本   → Hello（省略括号）\n  布尔   → true / false\n  空值   → null"
    )]
    PatchOp {
        /// 输入 PDF 文件路径
        #[arg(short, long)]
        input: PathBuf,
        /// 输出 PDF 文件路径
        #[arg(short, long)]
        output: PathBuf,
        /// 页码（从 1 开始）；与 --obj-id 互斥时优先
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 直接指定对象 ID（用于资源流/Form XObject 等），优先于 --page
        #[arg(long)]
        obj_id: Option<u32>,
        /// 操作符序号（来自 list-ops 输出的第一列，从 0 开始）
        #[arg(long)]
        index: usize,
        /// 校验操作符名称（不符时报错，防误操作，如 rg / Td / Tj）
        #[arg(long)]
        operator: Option<String>,
        /// 新操作数列表（按顺序传入）
        #[arg(long, num_args = 0..)]
        operands: Vec<String>,
        /// 针对 Form XObject 资源名（如 Fm0），而非页面内容流
        #[arg(long)]
        xobject: Option<String>,
    },
    /// 在内容流指定位置插入新操作符
    InsertOp {
        /// 输入 PDF 文件路径
        #[arg(short, long)]
        input: PathBuf,
        /// 输出 PDF 文件路径
        #[arg(short, long)]
        output: PathBuf,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 直接指定对象 ID（用于资源流等），优先于 --page
        #[arg(long)]
        obj_id: Option<u32>,
        /// 插入位置（在该序号之前插入，0 = 最开头，超出范围则追加到末尾）
        #[arg(long)]
        index: usize,
        /// 操作符名称（如 rg / Td / Tj / cm）
        #[arg(long)]
        operator: String,
        /// 操作数列表
        #[arg(long, num_args = 0..)]
        operands: Vec<String>,
        /// 针对 Form XObject 资源名（如 Fm0）
        #[arg(long)]
        xobject: Option<String>,
    },
    /// PDF 清理及安全净化：去权限/展平表单/删除交互/删签名/去注释/清元数据
    ///
    /// 不传任何 --xxx 开关时默认全部执行
    #[command(
        after_long_help = "示例:\n  全部清理:\n    pdf-surgeon sanitize -i a.pdf -o b.pdf\n\n  仅去除权限 + 删除签名:\n    pdf-surgeon sanitize -i a.pdf -o b.pdf --remove-perms --remove-sigs\n\n  删除注释并清除元数据:\n    pdf-surgeon sanitize -i a.pdf -o b.pdf --remove-annots --clean-meta"
    )]
    Sanitize {
        /// 输入 PDF 文件路径
        #[arg(short, long)]
        input: PathBuf,
        /// 输出 PDF 文件路径
        #[arg(short, long)]
        output: PathBuf,
        /// 去除加密权限限制（删除 Encrypt 字典及 Perms 标志）
        #[arg(long)]
        remove_perms: bool,
        /// 展平 AcroForm 表单（将表单字段外观合入页面内容，然后删除 AcroForm）
        #[arg(long)]
        flatten_forms: bool,
        /// 直接删除 AcroForm 表单（不展平，与 --flatten-forms 互斜时优先 flatten）
        #[arg(long)]
        remove_forms: bool,
        /// 去除所有页面及文档级交互动作（AA / OpenAction / URI / Launch 等）
        #[arg(long)]
        remove_actions: bool,
        /// 删除数字签名（/Sig 字段 + SigFlags）
        #[arg(long)]
        remove_sigs: bool,
        /// 删除所有页面注释（Annots）
        #[arg(long)]
        remove_annots: bool,
        /// 清除文档元数据（Info 字典 / Metadata 流 / Permissions）
        #[arg(long)]
        clean_meta: bool,
    },
    /// 去除 PDF 水印（支持八类：注释/文本/曲线/路径/痕迹/表单/图片/图案）
    ///
    /// 不传任何 --xxx 类型开关时，默认同时检测并去除所有八类水印。
    #[command(
        after_long_help = "示例:\n  去除所有类型水印:\n    pdf-surgeon remove-watermark -i a.pdf -o b.pdf\n\n  仅去除注释水印和图片水印:\n    pdf-surgeon remove-watermark -i a.pdf -o b.pdf --annot --image\n\n  用关键词精准匹配（只删含[机密]字样的水印）:\n    pdf-surgeon remove-watermark -i a.pdf -o b.pdf --keyword 机密\n\n  预览将要删除哪些内容（不写入文件）:\n    pdf-surgeon remove-watermark -i a.pdf -o b.pdf --dry-run\n\n  调低透明度阈值（默认 0.8，越小越保守）:\n    pdf-surgeon remove-watermark -i a.pdf -o b.pdf --opacity-threshold 0.5"
    )]
    RemoveWatermark {
        /// 输入 PDF 文件路径
        #[arg(short, long)]
        input: PathBuf,
        /// 输出 PDF 文件路径
        #[arg(short, long)]
        output: PathBuf,
        /// 页码（从 1 开始，默认处理第 1 页；与 --pages/--all-pages 互斥）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 页码范围，如 "1,3,5-8"（与 --all-pages 互斥）
        #[arg(long)]
        pages: Option<String>,
        /// 处理所有页面
        #[arg(long)]
        all_pages: bool,
        /// 去除【注释水印】：/Subtype Watermark、Stamp、FreeText 等注释
        #[arg(long)]
        annot: bool,
        /// 去除【文本水印】：BT…ET 块中透明/半透明文字（含 Tr 7 不可见文字）
        #[arg(long)]
        text: bool,
        /// 去除【曲线水印】：由贝塞尔曲线 (c/v/y) 组成的装饰花纹路径
        #[arg(long)]
        curve: bool,
        /// 去除【路径水印】：低透明度矩形/多边形填充路径（re/m/l + f/S）
        #[arg(long)]
        path: bool,
        /// 去除【痕迹水印】：单独 gs 引用极低透明度 ExtGState 的涂抹/印迹层
        #[arg(long)]
        trace: bool,
        /// 去除【表单水印】：AcroForm 中字段名含水印关键词的 Widget 字段
        #[arg(long)]
        form: bool,
        /// 去除【图片水印】：带 SMask（Alpha 通道）或名称含关键词的 XObject Image
        #[arg(long)]
        image: bool,
        /// 去除【图案水印】：内容含水印特征的 Form XObject（Do 调用）
        #[arg(long)]
        pattern: bool,
        /// 透明度判断阈值（0.0–1.0，低于此值视为水印层，默认 0.8）
        ///
        /// 例如设为 0.5 时，只有透明度 < 50% 的层才被识别为水印（更保守）；
        /// 设为 1.0 时，任何非完全不透明的层都会被识别（更激进）。
        #[arg(long, default_value_t = 0.8)]
        opacity_threshold: f32,
        /// 水印关键词（可多次传入），用于精准匹配文字/字段名/资源名中的水印标识
        ///
        /// 示例: --keyword 水印 --keyword CONFIDENTIAL
        /// 不传时使用内置默认关键词列表（watermark/水印/draft/草稿/confidential/机密等）
        #[arg(long, num_args = 1..)]
        keyword: Vec<String>,
        /// 预览模式：仅输出将被删除的内容，不写入文件
        #[arg(long)]
        dry_run: bool,

        /// 内容流搜索模式（多模式用 | 分隔），匹配到则删除包含该操作的 q…Q / BT…ET 块
        ///
        /// 支持三种格式：
        ///   ① 十六进制字节串  "226409A018411C5F"（匹配 Tj/TJ 字符串内容的十六进制表示）
        ///   ② 0x 前缀十六进制 "0x40BAFAD0A1C8BABDB2CA"（同上，以 0x 开头的字面量匹配）
        ///   ③ 操作符模式      "/Image5 Do" 或 "/KSPX* Do"（支持 * ? 通配符）
        ///
        /// 示例: --stream-search "226409A018411C5F|6211662F6C345370"
        #[arg(long)]
        stream_search: Option<String>,

        /// 同时搜索 XObject 资源内容流（配合 --stream-search 使用）
        ///
        /// 在 Form XObject 的内容流中找到匹配时，从页面内容流中删除对应的 Do 调用块
        #[arg(long)]
        search_resource_streams: bool,

        /// 按序号删除图像资源（XObject/Image），支持正序与倒序
        ///
        /// 格式：
        ///   "1,3-4"  → 删除第 1、3、4 个图像资源（1-based 正序）
        ///   "-1-3"   → 删除倒数第 1 至第 3 个图像资源（倒序范围）
        ///   "-1"     → 删除最后 1 个图像资源
        ///
        /// 示例: --res-del "-1-3"（删除倒数三个）
        #[arg(long)]
        res_del: Option<String>,

        /// 按序号跳过（保留）图像资源，配合 --res-del 或图片水印自动检测使用
        ///
        /// 格式与 --res-del 相同："-1" 表示保留最后一个，"1,3" 表示保留第 1、3 个
        ///
        /// 示例: --res-skip "-1"（删除所有图像资源，但保留最后一个）
        #[arg(long)]
        res_skip: Option<String>,
    },
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let password = cli.password;
    match cli.command {
        Commands::Extract {
            input,
            page,
            pages,
            all_pages,
            output,
        } => cmd::content::extract(input, page, pages, all_pages, output, &password)?,

        Commands::Import {
            input_pdf,
            output_pdf,
            page,
            stream_file,
            strip_whitespace,
        } => cmd::content::import(
            input_pdf,
            output_pdf,
            page,
            stream_file,
            strip_whitespace,
            &password,
        )?,

        Commands::ListRes {
            input,
            page,
            pages,
            all_pages,
            query,
            name,
            show_stream,
        } => cmd::resources::list_res(
            input,
            page,
            pages,
            all_pages,
            query,
            name,
            show_stream,
            &password,
        )?,

        Commands::TextInfo {
            input,
            page,
            all_pages,
        } => cmd::text::text_info(input, page, all_pages, &password)?,

        Commands::ListOps {
            input,
            page,
            category,
            op,
            stats,
            offsets,
            xobject,
            all_pages,
        } => cmd::content::list_ops(
            input, page, category, op, stats, offsets, xobject, all_pages, &password,
        )?,

        Commands::DeleteOps {
            input,
            output,
            page,
            range,
            skip,
        } => cmd::content::delete_ops(input, output, page, range, skip, &password)?,

        Commands::Replace {
            input,
            output,
            page,
            old,
            new,
            regex,
        } => cmd::content::replace_content(input, output, page, old, new, regex, &password)?,

        Commands::ListImages {
            input,
            page,
            pages,
            all_pages,
        } => cmd::resources::list_images(input, page, pages, all_pages, &password)?,

        Commands::ExtractImage {
            input,
            page,
            res_name,
            output,
        } => cmd::resources::extract_image(input, page, res_name, output, &password)?,

        Commands::HexView {
            input,
            page,
            pages,
            all_pages,
        } => cmd::content::hex_view(input, page, pages, all_pages, &password)?,

        Commands::ListAnnots {
            input,
            page,
            pages,
            all_pages,
        } => cmd::annots::list_annots(input, page, pages, all_pages, &password)?,

        Commands::Inspect {
            input,
            page,
            pages,
            all_pages,
            annots,
            resources,
            raw,
            text,
        } => cmd::text::inspect(
            input, page, pages, all_pages, annots, resources, raw, text, &password,
        )?,

        Commands::DelRes {
            input,
            output,
            page,
            res_type,
            res_name,
        } => cmd::resources::del_res(input, output, page, res_type, res_name, &password)?,

        Commands::Obj {
            input,
            id,
            trailer,
            catalog,
            stream,
        } => cmd::obj_view::obj(input, id, trailer, catalog, stream, &password)?,

        Commands::FontInfo {
            input,
            page,
            pages,
            all_pages,
            name,
            cmap,
            widths,
        } => {
            cmd::obj_view::font_info(input, page, pages, all_pages, name, cmap, widths, &password)?
        }

        Commands::PageInfo {
            input,
            page,
            set_box,
            rect,
            output,
            rotate,
            set_rotate,
        } => cmd::page::page_info(
            input, page, set_box, rect, output, rotate, set_rotate, &password,
        )?,

        Commands::Outline { input, depth } => cmd::document::outline(input, depth, &password)?,

        Commands::PageOp {
            input,
            output,
            delete,
            rotate,
            keep,
            reorder,
        } => cmd::page::page_op(input, output, delete, rotate, keep, reorder, &password)?,

        Commands::DocInfo { input } => cmd::document::doc_info(input, &password)?,

        Commands::Analyze {
            input,
            page,
            pages,
            all_pages,
        } => cmd::analyze::analyze(input, page, pages, all_pages, &password)?,

        Commands::Merge {
            inputs,
            pages,
            output,
        } => cmd::page::merge(inputs, pages, output, &password)?,

        Commands::SetObj {
            input,
            output,
            id,
            key,
            value,
        } => cmd::obj_view::set_obj(input, output, id, key, value, &password)?,

        Commands::PatchOp {
            input,
            output,
            page,
            obj_id,
            index,
            operator,
            operands,
            xobject,
        } => cmd::content::patch_op(
            input, output, page, obj_id, index, operator, operands, xobject, &password,
        )?,

        Commands::InsertOp {
            input,
            output,
            page,
            obj_id,
            index,
            operator,
            operands,
            xobject,
        } => cmd::content::insert_op(
            input, output, page, obj_id, index, operator, operands, xobject, &password,
        )?,

        Commands::Sanitize {
            input,
            output,
            remove_perms,
            flatten_forms,
            remove_forms,
            remove_actions,
            remove_sigs,
            remove_annots,
            clean_meta,
        } => cmd::document::sanitize(
            input,
            output,
            remove_perms,
            flatten_forms,
            remove_forms,
            remove_actions,
            remove_sigs,
            remove_annots,
            clean_meta,
            &password,
        )?,

        Commands::RemoveWatermark {
            input,
            output,
            page,
            pages,
            all_pages,
            annot,
            text,
            curve,
            path,
            trace,
            form,
            image,
            pattern,
            opacity_threshold,
            keyword,
            dry_run,
            stream_search,
            search_resource_streams,
            res_del,
            res_skip,
        } => cmd::watermark::remove_watermark(
            input,
            output,
            pages,
            all_pages,
            page,
            annot,
            text,
            curve,
            path,
            trace,
            form,
            image,
            pattern,
            opacity_threshold,
            keyword,
            dry_run,
            stream_search,
            search_resource_streams,
            res_del,
            res_skip,
            &password,
        )?,
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let stack_size = 8 * 1024 * 1024; // 8 MB
    let result = std::thread::Builder::new()
        .stack_size(stack_size)
        .spawn(run)?
        .join();
    match result {
        Ok(r) => r,
        Err(e) => std::panic::resume_unwind(e),
    }
}
