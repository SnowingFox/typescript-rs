# collections: 实现方案（impl.md）

**crate**：`tsgo_collections`　**目标**：提供编译器全程使用的容器：插入序 map/set（`OrderedMap`/`OrderedSet`）、普通 set（`Set`）、多值 map（`MultiMap`）、写时复制 map/set（`CopyOnWriteMap`/`CopyOnWriteSet`）、并发 map/set（`SyncMap`/`SyncSet`）。
**依赖（crate）**：`tsgo_json`（`OrderedMap` 的 JSON 编解码）。外部：`indexmap`、`rustc_hash`、`dashmap`。
**Go 源**：`internal/collections/`（7 个非测试文件：`ordered_map.go` 317、`ordered_set.go` 55、`set.go` 145、`multimap.go` 76、`cow.go` 77、`syncmap.go` 99、`syncset.go` 78 行）

## 这个包是什么（业务说明）

`collections` 是 typescript-go 的容器工具箱，几乎被所有上层包使用。挑容器的核心准则（也是移植的命门）是**确定性输出**：凡影响 emit/诊断顺序的集合必须保插入序。

- **`OrderedMap` / `OrderedSet`**：插入序 map/set，是诊断、符号表、emit 等需要稳定顺序处的主力。`OrderedMap` 还实现了 JSON 编解码（按插入序输出对象键）。
- **`Set`**：基于 `map[T]struct{}` 的普通集合，带 nil 安全的方法（`Has`/`Len`/`Clone` 对 nil receiver 返回零值）、集合代数（`Union`/`UnionedWith`/`Equals`/`IsSubsetOf`/`Intersects`）。
- **`MultiMap`**：一对多 map（`map[K][]V`），含 `GroupBy`。
- **`CopyOnWriteMap` / `CopyOnWriteSet`**：写时复制 + 作用域栈（`EnterScope` 返回还原闭包），用于 checker 推断时的可回滚作用域。
- **`SyncMap` / `SyncSet`**：基于 `sync.Map` 的并发容器（checker/project 并行用）。

## 所有权 / 类型映射（本包关键决策）

| Go 构造 | Rust 表示 | 说明 |
|---|---|---|
| `OrderedMap[K,V]`（`keys []K` + `mp map[K]V`） | `indexmap::IndexMap<K, V, FxBuildHasher>` | PORTING 指定映射；IndexMap 自带插入序 + O(1) 索引，替代手维护 `keys+mp` |
| `OrderedSet[T]` | `indexmap::IndexSet<T, FxBuildHasher>` | 同上 |
| `Set[T]`（`map[T]struct{}`） | `rustc_hash::FxHashSet<T>`（newtype 包装以挂方法） | 无序集合 + 集合代数 |
| `MultiMap[K,V]`（`map[K][]V`） | `FxHashMap<K, Vec<V>>`（newtype 包装） | 一对多 |
| `CopyOnWriteMap[K,V]`（`m`+`owned`） | 写时复制：`enum { Borrowed(Rc<HashMap>), Owned(HashMap) }` 或 `Rc<HashMap>` + `make_mut` | `owned` 标志 → Rust 用 `Rc::make_mut`（首次写自动克隆）或显式枚举；见所有权小节 |
| `SyncMap[K,V]`（`sync.Map`） | `dashmap::DashMap<K, V>` | PORTING §3：`sync.Map` → `dashmap` |
| `SyncSet[T]` | `dashmap::DashSet<T>` | 同上 |
| `iter.Seq[K]` / `iter.Seq2[K,V]`（Go 1.23 迭代器） | `impl Iterator<Item=...>` / `impl Iterator<Item=(K,V)>` | `Keys`/`Values`/`Entries` 返回迭代器 |
| `noCopy`（防拷贝标记） | Rust 所有权天然防拷贝 | 无需对应；`!Copy` 默认 |
| `(V, bool)` 返回（`Get`/`Delete`） | `Option<V>` / `Option<&V>` | 多返回值 → Option |
| `nil` receiver 安全（`Set`/`OrderedMap` 多方法） | `Option<&Self>` 或方法对空容器返回零值 | **偏离**：Go 的 nil-map 方法语义在 Rust 用"空容器"或 `Option` 表达，见偏离 |

### 所有权图：写时复制与作用域（CopyOnWriteMap）

Go 的 `CopyOnWriteMap` 用 `owned bool` 标志 + `EnterScope() func()`（保存当前 `*c`，把 `owned=false`，返回恢复闭包）实现"读共享、首写克隆、作用域可回滚"。Rust 两条路：

1. **`Rc<HashMap> + Rc::make_mut`**：`Set` 时 `Rc::make_mut(&mut self.m)` 自动在共享时克隆。`EnterScope` 保存 `Rc` 的克隆 + 恢复。零 `unsafe`、契合写时复制语义。
2. **作用域恢复**：Go 用闭包恢复 `*c`；Rust 用 RAII guard（`Drop` 时恢复）或返回一个 `ScopeGuard`，对齐 PORTING §4（`defer` → RAII）。

推荐方案 1 + RAII guard。该偏离写进本节即可（结构等价）。

## 文件清单 → Rust 模块

| Go 文件 | Rust 文件 | 说明 |
|---|---|---|
| `internal/collections/ordered_map.go` | `internal/collections/ordered_map.rs` | `OrderedMap` + JSON 编解码 + `DiffOrderedMaps` |
| `internal/collections/ordered_set.go` | `internal/collections/ordered_set.rs` | `OrderedSet`（包 OrderedMap） |
| `internal/collections/set.go` | `internal/collections/set.rs` | `Set` + 集合代数 |
| `internal/collections/multimap.go` | `internal/collections/multimap.rs` | `MultiMap` + `GroupBy` |
| `internal/collections/cow.go` | `internal/collections/cow.rs` | 写时复制 map/set |
| `internal/collections/syncmap.go` | `internal/collections/syncmap.rs` | `SyncMap` |
| `internal/collections/syncset.go` | `internal/collections/syncset.rs` | `SyncSet` |
| （crate 根） | `internal/collections/lib.rs` | 声明子模块 + re-export |

## 依赖白名单（本包新增的 crate）

- `indexmap`（OrderedMap/OrderedSet）、`rustc_hash`（FxHashMap/Set）、`dashmap`（SyncMap/SyncSet）。均在 PORTING §10 白名单内。
- `serde`（经 `tsgo_json` 间接；OrderedMap 的 JSON 编解码需自定义保序 `Serialize`/`Deserialize`）。
- 记到 `references/crate-map.md`。

## 实现 TODO（逐文件 / 逐函数，可勾选）

### `ordered_map.rs`（Go: `internal/collections/ordered_map.go`）

- [ ] `pub struct OrderedMap<K, V>(IndexMap<K, V, FxBuildHasher>)` + `MapEntry<K,V>{key,value}`　`// Go: ordered_map.go:OrderedMap/MapEntry`
- [ ] `pub fn with_size_hint(hint: usize) -> Self`　`// Go: ordered_map.go:NewOrderedMapWithSizeHint`
- [ ] `pub fn from_list(items: Vec<MapEntry<K,V>>) -> Self`　`// Go: ordered_map.go:NewOrderedMapFromList`
- [ ] `set / get -> Option<&V> / get_or_zero / entry_at(i) -> Option<(&K,&V)> / has / delete -> Option<V>`（delete 保序：用 `shift_remove` 保插入序，非 `swap_remove`）　`// Go: ordered_map.go:Set/Get/GetOrZero/EntryAt/Has/Delete`
- [ ] `keys() / values() / entries()` 迭代器（保插入序；支持迭代中追加的语义存疑见偏离）　`// Go: ordered_map.go:Keys/Values/Entries`
- [ ] `clear / size / clone`　`// Go: ordered_map.go:Clear/Size/Clone`
- [ ] `impl Serialize for OrderedMap`（按插入序写对象；key 解析对齐 `resolveKeyName`：string/TextMarshaler/整数）　`// Go: ordered_map.go:MarshalJSONTo/resolveKeyName`
- [ ] `impl Deserialize`（null → 空 no-op；非对象 → Err "cannot unmarshal non-object JSON value into Map"；逐键 Set 保序）　`// Go: ordered_map.go:UnmarshalJSONFrom`
- [ ] `pub fn diff_ordered_maps(...)`（V: PartialEq）/ `diff_ordered_maps_func(..., eq)` — 触发 onAdded/onRemoved/onModified 回调，遍历顺序：先 m2 找新增、再 m1 找修改/删除　`// Go: ordered_map.go:DiffOrderedMaps/DiffOrderedMapsFunc`

### `ordered_set.rs`（Go: `internal/collections/ordered_set.go`）

- [ ] `pub struct OrderedSet<T>(IndexSet<T, FxBuildHasher>)`　`// Go: ordered_set.go:OrderedSet`
- [ ] `with_size_hint / add / has / delete -> bool / values() / clear / size / clone`（delete 保序 `shift_remove`）　`// Go: ordered_set.go:*`

### `set.rs`（Go: `internal/collections/set.go`）

- [ ] `pub struct Set<T>(FxHashSet<T>)`　`// Go: set.go:Set`
- [ ] `with_size_hint / has / add / delete / len / keys / clear`　`// Go: set.go:*`
- [ ] `add_if_absent(key) -> bool`（未存在则加并返回 true）　`// Go: set.go:AddIfAbsent`
- [ ] `clone / union(&other) / unioned_with(&other) -> Set / equals / is_subset_of / intersects`　`// Go: set.go:Union/UnionedWith/Equals/IsSubsetOf/Intersects`
- [ ] `from_items(items) -> Set`　`// Go: set.go:NewSetFromItems`

### `multimap.rs`（Go: `internal/collections/multimap.go`）

- [ ] `pub struct MultiMap<K, V>(FxHashMap<K, Vec<V>>)`　`// Go: multimap.go:MultiMap`
- [ ] `with_size_hint / has / get -> &[V] / add / remove(key,value) / remove_all(key) / len / keys() / values() / clear`（remove：单值时删键，否则删元素保序）　`// Go: multimap.go:*`
- [ ] `pub fn group_by(items, group_id) -> MultiMap`　`// Go: multimap.go:GroupBy`

### `cow.rs`（Go: `internal/collections/cow.go`）

- [ ] `pub struct CopyOnWriteMap<K,V>` — `Rc<HashMap>` + 写时克隆（`Rc::make_mut`）　`// Go: cow.go:CopyOnWriteMap`
- [ ] `get -> Option<&V> / has / set`（set 触发写时克隆）　`// Go: cow.go:Get/Has/Set/ensureOwned`
- [ ] `enter_scope() -> ScopeGuard`（RAII；drop 时还原；对齐 Go 返回恢复闭包）　`// Go: cow.go:EnterScope`
- [ ] `pub struct CopyOnWriteSet<K>`（包 CopyOnWriteMap） + `has / add / enter_scope`　`// Go: cow.go:CopyOnWriteSet`

### `syncmap.rs`（Go: `internal/collections/syncmap.go`）

- [ ] `pub struct SyncMap<K,V>(DashMap<K,V>)`　`// Go: syncmap.go:SyncMap`
- [ ] `load -> Option<V> / store / load_or_store -> (V, bool) / delete / clear / range(f) / size / to_map / keys() / clone`（注意 nil-value 语义见 tests）　`// Go: syncmap.go:*`

### `syncset.rs`（Go: `internal/collections/syncset.go`）

- [ ] `pub struct SyncSet<T>(DashSet<T>)`　`// Go: syncset.go:SyncSet`
- [ ] `has / add / add_if_absent -> bool / delete / range / size / is_empty / to_slice / keys()`　`// Go: syncset.go:*`

### Cargo / crate 接线

- [ ] `internal/collections/Cargo.toml`（`name = "tsgo_collections"`，deps：`indexmap` `rustc_hash` `dashmap` `tsgo_json` path）
- [ ] 根 `Cargo.toml` workspace members 追加
- [ ] `lib.rs` 声明全部子模块 + re-export

## TDD 推进顺序（tracer bullet → 增量）

1. `OrderedMap`：set/get/has/size/delete（保序）+ keys/values/entries 迭代 —— 对齐 `TestOrderedMap` 全序列（tracer bullet，最大测试）。
2. `OrderedMap::clone` 独立性（`TestOrderedMapClone`）、`clear`（`TestOrderedMapClear`）、size-hint 不过度分配（`TestOrderedMapWithSizeHint`）。
3. `OrderedMap` JSON 编解码（`TestOrderedMapUnmarshalJSON`：对象/null/非对象错误/键类型错误）。
4. `OrderedSet`（`TestOrderedSet` + size-hint）。
5. `SyncMap` 的 nil-value 语义（`TestSyncMapWithNil`）。
6. `Set` 集合代数、`MultiMap`、`CopyOnWriteMap`/`Set`（Go 无直接单测，补行为级测试）。

## 与 Go 的已知偏离（divergence）

- **手维护 keys+map → IndexMap**：Go 用 `keys []K` + `mp map[K]V` 手维护插入序，且 `Delete` 对首/尾做了避免位移的优化。Rust 改用 `IndexMap`，**delete 必须用 `shift_remove`（保插入序）而非 `swap_remove`**，否则顺序断言失败。这是结构等价的允许偏离。
- **迭代中追加**：Go 的 `Keys/Values/Entries` 用 `for i:=0;i<len;i++` 显式支持"迭代时新增项也被枚举"。Rust 的 `IndexMap` 迭代器借用容器，**不允许迭代时修改**。若上层依赖此语义需改写为索引循环或先收集快照，标 `// TODO(port)`。当前测试不依赖该语义。
- **nil receiver 安全**：Go 的 `Set`/`OrderedMap` 多个方法对 nil receiver 返回零值（`Has`→false、`Size`→0、`Clone`→nil）。Rust 无 nil；用 `Option<Set>` 或空容器表达。集合代数（`Union`/`UnionedWith` 等）的 nil 组合分支需逐条用 `Option` 重写。
- **`sync.Map` 的 nil 值**：Go `SyncMap` 显式支持存 `nil` 值（`Load`/`LoadOrStore` 对 nil 特判，见 `TestSyncMapWithNil`）。Rust `V` 用 `Option<T>` 包装或 `DashMap<K, V>` 直接存——若 `V` 本身可空需用 `Option`。映射 `SyncMap[string, any]` 时 `any`→`Option<Box<dyn Any>>`，nil→`None`。
- **JSON key 解析（reflect）**：Go `resolveKeyName` 用 reflect 处理 string/TextMarshaler/整数键。Rust 用泛型 + trait bound（`Display`/自定义 `MapKey` trait）静态分发替代 reflect。
- **`noCopy`**：Go 防拷贝标记在 Rust 无需对应（所有权天然防移动后使用）。

## 转交 / 推迟（DEFER）

- `OrderedMap` 的完整 JSON 编解码（含 TextMarshaler 键、嵌套值）依赖 `tsgo_json` 的对应能力；若 json crate 流式 API 推迟，则保序序列化先用 `serde` 自定义实现。
- 迭代中可变（mutate-during-iteration）语义若被上层（binder/checker）需要，回填时标 `// DEFER(phase-3/4)`。
