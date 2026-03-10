# 🔪 PDF 瑞士军刀 - 精准的 PDF 手术工具

## 📖 简介

`pdf-knife` 是一把趁手的 PDF 瑞士军刀——它能让你像外科医生一样精准地解剖、诊断和处理 PDF 文件。

### ✨ 核心特性

| 特性 | 描述 |
|------|------|
| 🔍 **智能诊断** | 像 CT 扫描一样可视化 PDF 内部结构，自动识别水印和异常 |
| 🔪 **精准手术** | 操作符级别的手术刀，只切除目标，不伤及无辜 |
| 🛡️ **安全第一** | 预览模式、批量操作保护，误操作？不存在的 |
| 🚀 **性能卓越** | Rust 编写，内存安全，性能极致 |

---

## 🎯 使用场景

### 📚 **学生/研究人员**
```bash
# 下载的论文有水印？一键清除
pdf-knife remove-watermark -i paper.pdf -o clean.pdf

# 扫描版 PDF 文字无法复制？分析字体编码，看看问题出在哪
pdf-knife font-info paper.pdf --all-pages --cmap
pdf-knife text-info paper.pdf --page 1
```

### 💼 **职场人士**
```bash
# 收到的合同有"草稿"水印？一键去除
pdf-knife remove-watermark -i contract.pdf -o final.pdf

# 多个 PDF 需要合并整理？智能合并
pdf-knife merge a.pdf b.pdf c.pdf -o merged.pdf
```

### 🖥️ **开发者**
```bash
# PDF 内部结构分析？可视化报告
pdf-knife analyze document.pdf --all-pages

# 批量处理脚本？命令友好，易于集成
for file in *.pdf; do
    pdf-knife remove-watermark -i "$file" -o "clean_$file"
done
```

### 🔧 **普通用户**
```bash
# 先看看这个 PDF 里有什么
pdf-knife analyze document.pdf --page 1

# 让 AI 帮你决定怎么处理
# （配合 AI Agent 使用）
```

---

## 🚀 快速开始

### 安装

```bash
# 从源码编译
git clone https://github.com/zeromoul/pdf-knife
cd pdf-knife
cargo build --release
cp target/release/pdf-knife /usr/local/bin
```

### 一分钟上手

```bash
# 1. 看看这个 PDF 有什么问题
pdf-knife analyze document.pdf --page 1

# 2. 发现水印？一键清除
pdf-knife remove-watermark -i document.pdf -o clean.pdf

# 3. 检查清理结果
pdf-knife analyze clean.pdf --page 1
```

---

## 📋 命令详解

### 🎯 **一键智能处理**

| 命令 | 描述 | 示例 |
|------|------|------|
| `remove-watermark` | **自动识别并移除所有类型水印** | `pdf-knife remove-watermark -i in.pdf -o out.pdf` |
| `sanitize` | **PDF 清理及安全净化**（去权限/展平表单/删除交互/删签名/去注释/清元数据） | `pdf-knife sanitize -i in.pdf -o out.pdf` |
| `analyze` | **输出页面结构树形分析报告**（文字/图片/注释/表单/水印提示） | `pdf-knife analyze input.pdf --all-pages` |

### 🔍 **诊断分析工具**

| 命令 | 描述 | 示例 |
|------|------|------|
| `list-res` | **列出页面资源**（字体/图片/状态） | `pdf-knife list-res input.pdf --page 1` |
| `list-images` | **查看图片详情** | `pdf-knife list-images input.pdf --all-pages` |
| `list-annots` | **列出所有注释** | `pdf-knife list-annots input.pdf --page 1` |
| `font-info` | **字体编码分析**（诊断乱码原因） | `pdf-knife font-info input.pdf --page 1 --cmap` |
| `doc-info` | **文档基本信息**（版本/加密/元数据） | `pdf-knife doc-info input.pdf` |
| `outline` | **查看书签大纲** | `pdf-knife outline input.pdf --depth 3` |
| `obj` | **对象树浏览** | `pdf-knife obj input.pdf --catalog` |
| `hex-view` | **十六进制查看** | `pdf-knife hex-view input.pdf --page 1` |
| `text-info` | **查看页面文本信息及定位**（测试文字提取效果） | `pdf-knife text-info input.pdf --page 1` |
| `inspect` | **深度检查**（文本/注释/资源综合报告） | `pdf-knife inspect input.pdf --page 1 --text --annots` |
| `page-info` | **查看页面尺寸及旋转** | `pdf-knife page-info input.pdf --page 1` |

### 🛠️ **精准手术工具**

| 命令 | 描述 | 示例 |
|------|------|------|
| `extract` | **提取内容流** | `pdf-knife extract input.pdf --page 1 -o content.txt` |
| `import` | **导入内容流** | `pdf-knife import input.pdf output.pdf --page 1 --stream-file content.txt` |
| `list-ops` | **列出操作符**（支持分类/统计/偏移） | `pdf-knife list-ops input.pdf --page 1 --stats` |
| `delete-ops` | **删除指定操作符** | `pdf-knife delete-ops input.pdf output.pdf --page 1 --range 10 20` |
| `replace` | **文本替换**（支持正则） | `pdf-knife replace input.pdf output.pdf --page 1 --old "机密" --new ""` |
| `patch-op` | **修改操作数**（改颜色/位置/文本） | `pdf-knife patch-op -i in.pdf -o out.pdf --page 1 --index 42 --operator rg --operands 1.0 0 0` |
| `insert-op` | **插入操作符** | `pdf-knife insert-op -i in.pdf -o out.pdf --page 1 --index 0 --operator cm --operands 1 0 0 1 0 0` |
| `del-res` | **删除资源** | `pdf-knife del-res input.pdf output.pdf --page 1 --res-type XObject --res-name Im0` |
| `extract-image` | **提取图片** | `pdf-knife extract-image input.pdf --page 1 --res-name Im0 -o image.jpg` |
| `set-obj` | **修改对象字典** | `pdf-knife set-obj input.pdf output.pdf --id 123 --key ToUnicode --value delete` |

### 📦 **文档整理工具**

| 命令 | 描述 | 示例 |
|------|------|------|
| `merge` | **智能合并 PDF**（支持页码范围） | `pdf-knife merge a.pdf b.pdf --pages "1-3" "2,5" -o merged.pdf` |
| `page-op` | **页面操作**（删除/旋转/重排） | `pdf-knife page-op input.pdf output.pdf --delete 3 5 --rotate 1:90` |
| `page-info` | **设置页面尺寸/旋转**（带 --set-box / --set-rotate） | `pdf-knife page-info input.pdf --page 1 --set-box CropBox --rect 0 0 595 842 -o output.pdf` |

---

## 🔥 高级特性

### 🧠 **智能水印识别**

`pdf-knife` 能识别 **8 大类水印**：

| 类型 | 识别特征 | 开关 |
|------|----------|------|
| **注释水印** | `/Subtype Watermark/Stamp` + 关键词 | `--annot` |
| **文本水印** | `BT…ET` + 旋转矩阵 + 低透明度 | `--text` |
| **曲线水印** | 大量贝塞尔曲线 + 低透明度 | `--curve` |
| **路径水印** | 路径填充/描边 + 低透明度 | `--path` |
| **痕迹水印** | 极低透明度层 (ca<0.3) | `--trace` |
| **表单水印** | 字段名含水印关键词 | `--form` |
| **图片水印** | 图片有 SMask/ImageMask | `--image` |
| **图案水印** | `/PieceInfo/Watermark` + OCG 名称 | `--pattern` |

**最可靠的识别信号**：
```pdf
BDC /Artifact <</Subtype /Watermark/Type /Pagination>> ... EMC
```
> ✅ Adobe/WPS 标准水印标记，100% 准确！

### 🎛️ **可调参数**

```bash
# 透明度阈值（0.8 表示透明度 <80% 才视为水印）
--opacity-threshold 0.8

# 关键词匹配（支持多个）
--keyword 机密 --keyword CONFIDENTIAL

# 预览模式（只显示，不修改）
--dry-run

# 资源序号删除（支持正序/倒序）
--res-del "-1-3"  # 删除最后三个图片
--res-skip "-1"    # 保留最后一个
```

### 🔍 **内容流搜索**

```bash
# 十六进制字节串匹配
--stream-search "226409A018411C5F"

# 操作符模式匹配（支持 *? 通配符）
--stream-search "/Image5 Do|/KSPX* Do"

# 同时搜索 XObject 资源流
--search-resource-streams
```

### 🧹 **Sanitize 全面清理**

```bash
# 一键全部清理（去权限+展平表单+删动作+删签名+删注释+清元数据）
pdf-knife sanitize -i in.pdf -o out.pdf

# 选择性清理（只去权限 + 删签名 + 清元数据）
pdf-knife sanitize -i in.pdf -o out.pdf \
  --remove-perms \
  --remove-sigs \
  --clean-meta

# 展平表单 + 删除注释
pdf-knife sanitize -i in.pdf -o out.pdf \
  --flatten-forms \
  --remove-annots
```

---

## 📊 输出示例

### `analyze` 命令输出

```
📄 PDF 文档结构分析报告
═══════════════════════════════════════════════

第 1 页 (对象 ID: 5 0 R)
├─ 📦 页面属性
│  ├─ MediaBox: [0 0 595 842] (A4)
│  └─ Rotate: 0°
│
├─ 📝 文字内容 (共 3 段)
│  ├─ 段 1: "年度财务报告"
│  │    ├─ 字体: /F1
│  │    ├─ 大小: 24pt
│  │    ├─ 位置: (200, 750)
│  │    └─ 透明度: 100%
│  ├─ 段 2: "机密"
│  │    ├─ 字体: /F2
│  │    ├─ 大小: 72pt
│  │    ├─ 位置: (200, 400)
│  │    └─ 透明度: 30%  ⚠️ 半透明
│  └─ 段 3: "2024年1月"
│       ├─ 字体: /F1
│       ├─ 大小: 12pt
│       ├─ 位置: (400, 100)
│       └─ 透明度: 100%
│
├─ 🖼️  图片资源 (共 2 个)
│  ├─ /Im0 (ID: 10 0 R)
│  │    ├─ 类型: JPEG
│  │    ├─ 像素: 800×600
│  │    ├─ 大小: 245 KB
│  │    ├─ 位置: (0, 0)  页面尺寸: 595×842pt
│  │    └─ 颜色空间: RGB  ⚠️ 含SMask透明通道
│  └─ /Im1 (ID: 12 0 R)
│       ├─ 类型: PNG
│       ├─ 像素: 150×50
│       ├─ 大小: 8 KB
│       ├─ 位置: (400, 500)  页面尺寸: 150×50pt
│       └─ 颜色空间: RGB
│
├─ � 表单字段 (共 0 个)
│
├─ 💬 注释 (共 0 个)
│
├─ �🔧 图形状态 (含透明度设置 2 个)
│  ├─ /GS2: fill=50%  stroke=50%  ⚠️ 疑似水印透明度
│  └─ /GS3: fill=30%  stroke=30%  ⚠️ 疑似水印透明度
│
└─ 📊 统计信息
   ├─ 总操作符数: 156
   ├─ 文字操作:   45
   ├─ 路径操作:   78
   ├─ 颜色操作:   12
   ├─ 状态变更:   15
   ├─ XObject:    5
   └─ 标记内容:   1

⚠️ 检测到可能的水印:
   - 文字 "机密" 透明度 30%
   - 图形状态 /GS2 透明度 50%，疑似水印层
   - 图片 /Im0 含透明通道 (SMask)，疑似图片水印
```

---


## 📄 许可证

MIT License © 2026 molin

---

## 🙏 致谢

- [lopdf](https://github.com/J-F-Liu/lopdf) - Rust PDF 库

---

<div align="center">

**如果这个工具帮到了你，请给它一个 ⭐️**

[GitHub](https://github.com/zeromoul/pdf-knife) • [Issues](https://github.com/zeromoul/pdf-knife/issues) • [Discussions](https://github.com/zeromoul/pdf-knife/discussions)

</div>