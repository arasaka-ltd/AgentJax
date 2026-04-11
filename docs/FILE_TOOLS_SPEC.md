# AgentJax File Tools Spec
## 1. 目标
本文档定义 AgentJax 面向 Agent 的文件操作工具协议。

目标不是只给 Agent 一个粗糙的 `read_file` 或 `shell` 替代品。
而是正式定义一组足够稳定、可控、可演进的文件工具，让 Agent 可以直接参与：
- 项目开发
- 代码维护
- 配置修改
- prompt / identity 文件维护
- memory / knowledge 库维护

第一阶段先定义三个核心工具：
- `read`
- `edit`
- `write`

---
## 2. 一句话边界
- `read`：读取指定路径下的文件内容或片段
- `edit`：对已有文本文件做精确修改
- `write`：创建或覆盖文本文件，并可自动创建缺失目录

这三个工具构成 Agent 直接参与项目开发与维护的最小文件操作闭环。

---
## 3. 设计原则
### 3.1 文件工具应是 Agent 的一等能力
Agent 不应被迫依赖 `shell` 才能完成常见文件操作。

也就是说：
- 看文件，用 `read`
- 改已有文本，用 `edit`
- 新建或整体写入文件，用 `write`

`shell` 可以保留，但不应成为默认文件操作入口。

### 3.2 `read` / `edit` / `write` 必须语义分离
- `read` 只负责读取
- `edit` 只负责修改已有文本文件
- `write` 负责新建或整体写入文本文件

不要把这三个动作混成一个万能 `file` 工具。
否则：
- 权限边界会模糊
- tool 选择会混乱
- 程序侧难以做审计、回放和安全控制

### 3.3 文本工具必须显式理解位置
只支持“整文件字符串替换”是不够的。

文件工具应支持：
- 行号范围
- 行内列位置
- 字符范围
- 精确插入、替换、删除

这样 Agent 才能可靠参与工程维护，而不是靠模糊重写全文。

### 3.4 `\n` 是核心语义，不是 incidental formatting
语言模型天然频繁使用 `\n` 表达多行文本。

因此协议必须明确：
- 工具输入输出统一用 `\n` 表达逻辑换行
- 程序侧应显式处理目标文件的换行风格
- 不能让模型自己猜“这个文件到底是 LF 还是 CRLF”

### 3.5 图片读取是 `read` 的一部分，但编辑不在本规范内
第一阶段：
- `read` 支持文本和图片
- `edit` / `write` 只针对文本文件

图片修改、裁切、重绘、生成不属于本规范，应由独立 image tool surface 负责。

---
## 4. Tool Catalog
第一阶段最小闭环：
- `read`
- `edit`
- `write`

---
## 5. `read`
### 5.1 作用
读取指定路径下的文件内容。

支持：
- 文本文件全文读取
- 文本文件按行读取
- 文本文件精确片段读取
- 图片文件读取为模型可理解的结构化结果

### 5.2 输入
```json
{
  "path": "src/main.rs",
  "start_line": 10,
  "end_line": 40
}
```

### 5.3 输入字段定义
- `path: string`
  - 必填
  - 文件路径
- `start_line?: integer`
  - 可选
  - 1-based inclusive
- `end_line?: integer`
  - 可选
  - 1-based inclusive
- `start_column?: integer`
  - 可选
  - 与 `start_line` 一起使用
- `end_column?: integer`
  - 可选
  - 与 `end_line` 一起使用
- `max_lines?: integer`
  - 可选
  - 读取预算保护
- `encoding?: string`
  - 可选
  - 默认建议 `utf-8`

约束：
- 文本文件可按行列读取
- 图片文件忽略行列参数
- 如果只给 `start_line` 不给 `end_line`，实现可按 `max_lines` 推导窗口

### 5.4 文本输出
```json
{
  "path": "src/main.rs",
  "kind": "text",
  "content": "10| fn main() {\n11|     println!(\"hi\");",
  "start_line": 10,
  "end_line": 11,
  "total_lines": 120,
  "newline": "lf",
  "truncated": false
}
```

字段建议：
- `kind: "text"`
- `content`
- `start_line`
- `end_line`
- `total_lines`
- `newline`
  - `lf`
  - `crlf`
  - `mixed`
- `truncated`

### 5.5 图片输出
```json
{
  "path": "assets/logo.png",
  "kind": "image",
  "mime_type": "image/png",
  "width": 512,
  "height": 512,
  "size_bytes": 18342,
  "image_ref": "image:workspace/assets/logo.png"
}
```

字段建议：
- `kind: "image"`
- `mime_type`
- `width`
- `height`
- `size_bytes`
- `image_ref`

说明：
- `read` 对图片的职责是“让模型能读到这个图片文件”
- 不要求把图片编辑能力塞进同一个工具里

### 5.6 使用策略
- 需要看文件内容时调用
- 已知只需局部文本时，优先按行读取
- 需要看图片文件时调用同一个 `read`

---
## 6. `edit`
### 6.1 作用
对已有文本文件做精确修改。

它用于：
- 插入
- 替换
- 删除
- 局部重写

不用于：
- 新建文件
- 图片文件编辑
- 二进制修改

### 6.2 设计要求
`edit` 必须足够精确，至少支持：
- 按行范围修改
- 按行列范围修改
- 在某个位置插入文本
- 删除指定区间

### 6.3 输入模型
第一阶段建议统一为区间编辑模型：
```json
{
  "path": "src/main.rs",
  "start_line": 12,
  "start_column": 5,
  "end_line": 12,
  "end_column": 18,
  "new_text": "println!(\"hello\");"
}
```

### 6.4 输入字段定义
- `path: string`
  - 必填
  - 目标文本文件
- `start_line: integer`
  - 必填
  - 1-based inclusive
- `start_column: integer`
  - 必填
  - 1-based inclusive
- `end_line: integer`
  - 必填
  - 1-based exclusive-bound line coordinate
- `end_column: integer`
  - 必填
  - 1-based exclusive
- `new_text: string`
  - 必填
  - 替换到该区间的新文本

正式建议：
- `start_*` 为包含起点
- `end_*` 为排他终点

这样更接近常见编辑器与编程语言 slice 语义，更容易避免 off-by-one。

### 6.5 操作语义
如果：
- 起点 < 终点，且 `new_text` 非空：替换
- 起点 < 终点，且 `new_text` 为空：删除
- 起点 == 终点，且 `new_text` 非空：插入

### 6.6 输出
```json
{
  "path": "src/main.rs",
  "applied": true,
  "newline": "lf",
  "new_range": {
    "start_line": 12,
    "start_column": 5,
    "end_line": 12,
    "end_column": 23
  }
}
```

### 6.7 换行规则
`new_text` 中的逻辑换行统一使用 `\n`。

程序侧建议：
- 先把 `new_text` 按逻辑层理解为 LF
- 再根据目标文件原有 newline style 写回：
  - 若原文件是 LF，则保持 LF
  - 若原文件是 CRLF，则写回 CRLF
  - 若原文件混用，可选择归一化或拒绝，具体由实现策略决定

### 6.8 前置条件
- 目标路径必须存在
- 目标必须是文本文件
- 默认不负责创建缺失目录或文件

这些工作属于 `write`。

### 6.9 使用策略
- 已有文件做局部改动时优先使用
- 尽量不要为了改几行而整体覆写全文
- 需要严格控制 patch 范围时使用

---
## 7. `write`
### 7.1 作用
创建或整体写入文本文件。

适用于：
- 新建文件
- 覆盖写入
- 批量生成初始化内容
- 在目录不存在时连目录一起创建

### 7.2 输入
```json
{
  "path": "knowledge/rust/notes/ownership.md",
  "content": "# Ownership\n\nRust ownership rules...",
  "create_dirs": true,
  "overwrite": false
}
```

### 7.3 输入字段定义
- `path: string`
  - 必填
- `content: string`
  - 必填
  - 允许为空字符串，但不要求必须为空
- `create_dirs?: boolean`
  - 可选
  - 为 `true` 时，允许类似 `mkdir -p`
- `overwrite?: boolean`
  - 可选
  - 为 `false` 时，如果文件已存在则失败
- `encoding?: string`
  - 可选
  - 默认建议 `utf-8`
- `newline?: string`
  - 可选
  - 建议值：
    - `preserve_if_exists`
    - `lf`
    - `crlf`

### 7.4 行为语义
- 如果文件不存在：
  - `create_dirs=true` 时允许自动创建父目录
  - 否则父目录不存在应失败
- 如果文件已存在：
  - `overwrite=true` 时允许覆盖
  - `overwrite=false` 时应失败

### 7.5 输出
```json
{
  "path": "knowledge/rust/notes/ownership.md",
  "created": true,
  "overwritten": false,
  "bytes_written": 128,
  "newline": "lf"
}
```

### 7.6 换行规则
`content` 中的逻辑换行统一使用 `\n`。

写入策略建议：
- 新文件默认写成 LF
- 已存在文件且 `newline=preserve_if_exists` 时，继承原文件换行风格
- 若显式指定 `lf` 或 `crlf`，则按该风格写出

### 7.7 使用策略
- 创建新文件时使用
- 需要整体写入稳定内容时使用
- 需要自动建目录时使用

---
## 8. 文本坐标与区间规范
### 8.1 行号
- 1-based

### 8.2 列号
- 1-based

### 8.3 区间
正式建议：
- 起点 inclusive
- 终点 exclusive

即：
- `start_line,start_column` 包含
- `end_line,end_column` 不包含

这是为了让：
- 插入操作更自然
- 替换区间更可组合
- 程序内部更容易映射到 slice 语义

### 8.4 字符语义
第一阶段建议按 Unicode 标量值或 Rust/UTF-8 解码后的字符位置定义，而不是按字节偏移暴露给 Agent。

不要让 Agent 直接处理 byte offset。
因为：
- 对中文不友好
- 对多字节字符容易出错
- 不适合作为通用 Agent-facing 协议

---
## 9. `\n` 与换行风格规范
这是这组工具里必须明确拍死的点。

### 9.1 Agent-facing 输入
Agent 在 `new_text` / `content` 中使用的换行统一视为逻辑 `\n`。

### 9.2 Runtime-facing 处理
runtime 必须负责：
- 识别目标文件换行风格
- 在需要时把逻辑 `\n` 转成目标风格
- 在读取时告诉 Agent 当前文件的换行风格

### 9.3 为什么必须这样设计
因为语言模型天然倾向于：
- 在 JSON 字符串里用 `\n`
- 以 LF 心智生成多行文本

如果协议不明确，最后会出现：
- 模型以为自己写的是多行，程序当成一行
- 程序把 LF 和 CRLF 混写
- patch 位置计算发生偏移

---
## 10. 核心文件与高权限编辑边界
AgentJax 的目标不是“只能读代码”，而是“可以直接参与项目开发与维护”，甚至维护自己的核心文件与知识域。

因此文件工具原则上应允许操作：
- 源代码
- 配置文件
- prompt / identity 文件
- `MEMORY.md`
- `memory/**`
- `knowledge/**`

但必须明确风险分层。

### 10.1 普通工程文件
例如：
- `src/**`
- `tests/**`
- `docs/**`
- `config examples`

允许常规读写。

### 10.2 自我与长期知识文件
例如：
- `AGENT.md`
- `SOUL.md`
- `USER.md`
- `MISSION.md`
- `RULES.md`
- `ROUTER.md`
- `MEMORY.md`
- `memory/**`
- `knowledge/**`

这些文件允许编辑，但应支持更严格的 policy：
- 可审计
- 可回溯
- 可选 require-confirmation
- 可接 hook 做冲突检测、policy 检查、memory promotion 校验

### 10.3 不建议默认放开的目标
例如：
- 密钥文件
- runtime secrets
- 外部系统凭据
- 超出 workspace 边界的敏感路径

这些应由单独的权限模型控制，而不是只靠 tool 自身。

---
## 11. Agent 使用策略
### 11.1 什么时候用 `read`
- 需要查看文本文件内容
- 需要读取图片文件
- 需要按行确认某段代码、文档或配置

### 11.2 什么时候用 `edit`
- 已有文本文件做局部修改
- 需要精确到行列范围
- 需要最小变更而不是整体重写

### 11.3 什么时候用 `write`
- 新建文件
- 整体生成文件
- 目录不存在，需要连父目录一起创建

### 11.4 不推荐行为
- 不要用 `write` 去改一个只需局部修改的大文件
- 不要用 `edit` 去承担“创建新文件”的职责
- 不要默认退回 `shell` 做普通文件编辑

---
## 12. 后续扩展方向
这三个工具是最小闭环，不是终局。

后续可扩展：
- `append`
- `move`
- `copy`
- `delete`
- `mkdir`
- `image.edit`
- `patch.apply`
- 多文件事务写入
- dry-run / diff preview

但第一阶段先把 `read` / `edit` / `write` 的协议与换行语义拍死。
