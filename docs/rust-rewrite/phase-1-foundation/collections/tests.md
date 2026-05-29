# collections: 测试清单（tests.md）

**完成列**：`✓`=Rust 已有对应 `#[test]` 且 `cargo test` 通过；留空=未写/未过；`—`=推迟到指定 phase。
**Go 测试规模**：3 文件 / 8 顶层 `func Test` / 含 1 处 `t.Run` 子用例（`TestOrderedMapUnmarshalJSON/UnmarshalJSONV2`）。其余为命令式（非表驱动）长测试，下表按其内部断言序拆成多行。

> Go 测试在 `package collections_test`，用 `gotest.tools/v3/assert`。`set.go`/`multimap.go`/`cow.go`/`syncset.go` **无直接单测**（行为由 P10 兜底 + 本轮补行为级测试）。

## 测试文件 → Rust 测试模块

| Go 测试文件 | Rust 测试位置 | 顶层测试函数数 |
|---|---|---|
| `internal/collections/ordered_map_test.go` | `internal/collections/ordered_map.rs`（`#[cfg(test)] mod tests`） | 5 |
| `internal/collections/ordered_set_test.go` | `internal/collections/ordered_set.rs` | 2 |
| `internal/collections/syncmap_test.go` | `internal/collections/syncmap.rs` | 1 |

## `ordered_map_test.go`

> `TestOrderedMap` 是命令式长测试（N=1000，升序填充），按断言序拆行。辅助 `padInt(n)` = `fmt.Sprintf("%10d", n)`（右对齐 10 宽）。

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `ordered_map_empty_has` | 空 map `has` 为假 | `m.has(1)` → false | `ordered_map_test.go:TestOrderedMap` | |
| `ordered_map_set_and_size` | 填 N 个键后 size=N | set 1..=1000 → `size()==1000` | `ordered_map_test.go:TestOrderedMap` | |
| `ordered_map_overwrite_keeps_size` | 反序覆盖已有键 size 不变 | 反向再 set → `size()==1000` | `ordered_map_test.go:TestOrderedMap` | |
| `ordered_map_get_all` | 逐键 get 命中且值正确 | `get(i)` → Some(`padInt(i)`) | `ordered_map_test.go:TestOrderedMap` | |
| `ordered_map_entries_values` | entries 值与键对应 | 遍历 `entries()` → `v==padInt(k)` | `ordered_map_test.go:TestOrderedMap` | |
| `ordered_map_keys_sorted` | keys 保插入序（升序填充→升序） | `keys` 长 N 且已排序 | `ordered_map_test.go:TestOrderedMap` | |
| `ordered_map_values_sorted` | values 保插入序 | `values` 长 N 且已排序 | `ordered_map_test.go:TestOrderedMap` | |
| `ordered_map_first_key_value` | 首个 key/value 是 start | 首 key=1、首 value=`padInt(1)` | `ordered_map_test.go:TestOrderedMap` | |
| `ordered_map_delete_middle` | 删中段键，get 返回 None，重复删返回 None | delete 2..1000 → ok 一次、再删 None、get None（值空串） | `ordered_map_test.go:TestOrderedMap` | |
| `ordered_map_delete_to_empty` | 删到只剩 start 再删空 | 删后 size=1→has(start)→删 start→size=0 | `ordered_map_test.go:TestOrderedMap` | |
| `ordered_map_clone_independent` | clone 与原独立 | clone≠m、size=2、keys=[1,2]、values=["one","two"]；删原 m 后 clone 不变 | `ordered_map_test.go:TestOrderedMapClone` | |
| `ordered_map_clear` | clear 后 size=0 | set 两项→clear→`size()==0` | `ordered_map_test.go:TestOrderedMapClear` | |
| `ordered_map_size_hint_allocs` | size-hint 下分配 < 10 次 | `with_size_hint(1024)` 填 1024 → allocs<10 | `ordered_map_test.go:TestOrderedMapWithSizeHint` | |
| `ordered_map_unmarshal_object` | 反序列化对象保序 | `{"a":1,"b":"two","c":{"d":4}}` → size=3、`get_or_zero("a")`==1.0 | `ordered_map_test.go:TestOrderedMapUnmarshalJSON/UnmarshalJSONV2` | |
| `ordered_map_unmarshal_null_noop` | null 为 no-op | `null` → Ok(无变化) | `ordered_map_test.go:TestOrderedMapUnmarshalJSON/UnmarshalJSONV2` | |
| `ordered_map_unmarshal_non_object_err` | 非对象报错 | `"foo"` → Err 含 `"cannot unmarshal non-object JSON value into Map"` | `ordered_map_test.go:TestOrderedMapUnmarshalJSON/UnmarshalJSONV2` | |
| `ordered_map_unmarshal_bad_key_type_err` | 键类型不符报错 | `OrderedMap<int,_>` 解析 `{"a":1,...}` → Err 含 `"unmarshal"` | `ordered_map_test.go:TestOrderedMapUnmarshalJSON/UnmarshalJSONV2` | |

## `ordered_set_test.go`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `ordered_set_add_has_delete` | add/has/delete + 保序 values | add 1,2,3→has 全真→delete(2)→true；values 长 2 且有序 | `ordered_set_test.go:TestOrderedSet` | |
| `ordered_set_clear_and_clone` | clear 后空、clone 独立空 | clear→size=0、has 全假；clone≠s、size=0 | `ordered_set_test.go:TestOrderedSet` | |
| `ordered_set_size_hint_allocs` | size-hint 下分配 < 10 次 | `with_size_hint(1024)` 加 1024 → allocs<10 | `ordered_set_test.go:TestOrderedSetWithSizeHint` | |

## `syncmap_test.go`

| Rust 测试 | 验证内容 | input → expected | Go 对照 | 完成 |
|---|---|---|---|---|
| `sync_map_with_nil_load_miss` | 未存在 load miss | `load("foo")` → (None/nil, false) | `syncmap_test.go:TestSyncMapWithNil` | |
| `sync_map_store_nil_then_load` | 存 nil 值后 load 命中且为 nil | store("foo", nil)→load → (nil, true) | `syncmap_test.go:TestSyncMapWithNil` | |
| `sync_map_load_or_store_nil` | load_or_store nil 未加载 | `load_or_store("too", nil)` → (nil, loaded=false) | `syncmap_test.go:TestSyncMapWithNil` | |
| `sync_map_range_noop` | range 遍历不崩 | `range(|_,_| true)` 正常 | `syncmap_test.go:TestSyncMapWithNil` | |

## 0 直接单测的情况（补充行为级 Rust 测试）

`set.go` / `multimap.go` / `cow.go` / `syncset.go` Go 侧无直接单测，行为由 **P10 parity** 兜底。本轮补行为级测试（expected 取自 Go 实现逻辑）：

| Rust 测试 | 验证内容 | input → expected | 依据 | 完成 |
|---|---|---|---|---|
| `set_add_has_delete_len` | 基本增删查 | add a,b→has a true、len 2、delete a→has a false | set.go:Add/Has/Delete/Len | |
| `set_add_if_absent` | 缺则加返回 true | `add_if_absent(x)`→true、再次→false | set.go:AddIfAbsent | |
| `set_union_and_unioned_with` | 并集（原地/新建） | {a}∪{b}={a,b}；`unioned_with` 不改原 | set.go:Union/UnionedWith | |
| `set_equals_subset_intersects` | 相等/子集/相交 | {a,b}.equals({a,b})→true；{a}.is_subset_of({a,b})→true；{a}.intersects({a})→true、{a}.intersects({b})→false | set.go:Equals/IsSubsetOf/Intersects | |
| `set_from_items` | 变参构造去重 | `from_items(a,a,b)` → len 2 | set.go:NewSetFromItems | |
| `multimap_add_get_remove` | 一对多增删 | add(k,1),add(k,2)→get(k)=[1,2]；remove(k,1)→[2]；remove 最后一个→键消失 | multimap.go:Add/Get/Remove | |
| `multimap_group_by` | 分组 | `group_by([1,2,3,4], |v| v%2)` → {0:[2,4],1:[1,3]} | multimap.go:GroupBy | |
| `cow_map_read_shared_write_clones` | 写时复制 | 子作用域读见父项；写后父不变 | cow.go:CopyOnWriteMap/Set/ensureOwned | |
| `cow_enter_scope_restores` | 作用域回滚 | enter_scope→改动→guard drop→还原 | cow.go:EnterScope | |
| `cow_set_basic` | CopyOnWriteSet 增/查 | add(k)→has(k) true；作用域回滚生效 | cow.go:CopyOnWriteSet | |
| `sync_set_add_if_absent` | 并发 set 幂等加 | `add_if_absent(k)`→true、再次→false | syncset.go:AddIfAbsent | |
| `sync_set_size_is_empty_to_slice` | size/is_empty/to_slice | 空→is_empty true；加 2→size 2、to_slice 含两项 | syncset.go:Size/IsEmpty/ToSlice | |

## 与 impl.md 的对齐核对

- [x] 每个 Go `func Test*` 都已映射（8 个：OrderedMap×5 + OrderedSet×2 + SyncMap×1）
- [x] `t.Run` 子用例（`UnmarshalJSONV2`）的内部断言逐条列出
- [x] 命令式长测试按断言序拆行（TestOrderedMap 拆 10 行等）
- [x] expected 值均取自 Go 测试字面量（padInt 格式、错误消息、size 数值）
- [x] 每条带 `// Go:` 锚点
- [x] 与 impl.md 双向对齐：补充测试覆盖的 set/multimap/cow/syncset 在 impl.md 均有 TODO

## 推迟到后续 phase 的测试

| 测试 / 行为 | 原因 | 目标 phase |
|---|---|---|
| 迭代中追加（mutate-during-iteration）语义 | IndexMap 不支持，上层若依赖需改写 | P3/P4 回填 |
| `SyncMap`/`SyncSet` 真并发竞争正确性 | 需并发压测，dashmap 行为对齐 | 实现期 + P10 |
| `OrderedMap` 序列化与 Go 字节级一致（含 TextMarshaler 键） | 需 tsgo_json 完整能力 | P10 parity |
| `CopyOnWriteMap` 在 checker 作用域回滚的真实用法 | 需 checker（P4） | P4 |
