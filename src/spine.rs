use core::cmp::min;
use core::ffi::c_void;

// Offsets::ProcessEventIdx
const PROCESSEVENT_IDX: usize = 0x4C;
// UHTUI_CharacterAwaken presentation members.
const OFF_AWAKEN_SPINE: usize = 0x1500;
// UHTUI_CharacterAwakenSpine presentation-only widget members.
const OFF_AWAKEN_IMAGE_GRAY: usize = 0x0F98;
const OFF_AWAKEN_IMAGE_NORMAL: usize = 0x0FA0;
const OFF_AWAKEN_SPINE_CHARACTER: usize = 0x0FA8;
const OFF_AWAKEN_SPINE_EFFECT: usize = 0x0FB0;
// UHTUIBase::bIsClosing / UWidget::Visibility.
const OFF_UI_CLOSING: usize = 0x061A;
const OFF_WIDGET_VISIBILITY: usize = 0xDC;
const VISIBLE: u8 = 0;
const COLLAPSED: u8 = 1;

// ---- object / array / FName ----
const OFF_MAX_ELEMENTS: usize = 0x10;
const OFF_NUM_ELEMENTS: usize = 0x14;
const OFF_MAX_CHUNKS: usize = 0x18;
const OFF_NUM_CHUNKS: usize = 0x1C;
const ELEMENTS_PER_CHUNK: u32 = 0x10000;
const FUOBJECT_ITEM: usize = 0x18;
const UA_MIN_ELEMENTS: u32 = 0x800;
const UA_MIN_MAX_ELEMENTS: u32 = 0x1_0000;
const UA_MIN_CHUNKS: u32 = 1;
const UA_MAX_CHUNKS: u32 = 0x14;
const UA_MIN_MAX_CHUNKS: u32 = 6;
const UA_MAX_MAX_CHUNKS: u32 = 0x5FF;
const UA_CHUNK_ALIGN: u32 = 0x10;
const UA_MIN_PER_CHUNK: u32 = 0x8000;
const UA_MAX_PER_CHUNK: u32 = 0x8_0000;
const FNAME_BLOCKS_MAX: usize = 8192;
const FNAME_BLOCK_BYTES_MAX: usize = 0x2_0000;

const MAX_ELEMENTS: u32 = 8_000_000;
const MAX_CHUNKS: u32 = 0x1000;
const MAX_SUPER_DEPTH: usize = 64;
// Only persistent UI-subsystem acquisition visits GObjects, in bounded batches.
const UI_MANAGER_SCAN_BATCH: u32 = 32_768;

const POOL_BLOCKS: usize = 0x10;
const BLOCK_SHIFT: u32 = 16;
const BLOCK_MASK: u32 = 0xFFFF;
const ENTRY_HEADER: usize = 2;
const MAX_NAME: usize = 64;

const AWAKEN_CLASS: &[u8] = b"HTUI_CharacterAwaken";
const AWAKEN_SPINE_CLASS: &[u8] = b"HTUI_CharacterAwakenSpine";
const CHARACTER_MAIN_CLASS: &[u8] = b"HTUI_CharacterMainForm";
const CHARACTER_INFO_CLASS: &[u8] = b"HTUI_CharacterInfo";
const CONTAINER_CLASS: &[u8] = b"CommonActivatableWidgetContainerBase";
const OVERALL_LAYOUT_CLASS: &[u8] = b"W_OverallUILayout_C";
const GAME_INSTANCE_CLASS: &[u8] = b"GameInstance";
const UI_MANAGER_CLASS: &[u8] = b"HTUIManagerSubsystem";
const POLICY_CLASS: &[u8] = b"GameUIPolicy";
// UGameUIManagerSubsystem::CurrentPolicy / UGameUIPolicy::RootViewportLayouts.
const OFF_UI_MANAGER_POLICY: usize = 0x30;
const OFF_POLICY_ROOT_LAYOUTS: usize = 0x58;
// FRootViewportLayoutInfo (CommonGame_structs.hpp): LocalPlayer @0, RootLayout @8, Added @16.
const ROOT_LAYOUT_STRIDE: usize = 0x18;
const OFF_ROOT_LAYOUT: usize = 0x08;
const OFF_ROOT_ADDED_TO_VIEWPORT: usize = 0x10;
const MAX_UI_ROOTS: u32 = 8;
const OFF_MAIN_CHARACTER_INFO: usize = 0x1010;
const OFF_INFO_AWAKEN: usize = 0x1620;
// UCommonActivatableWidgetContainerBase / UPrimaryGameLayout (CommonUI/CommonGame SDK).
const OFF_CONTAINER_DISPLAYED: usize = 0x1A0;
const OFF_LAYOUT_LAYERS: usize = 0x390;
// TMap<FGameplayTag(8), UObject*(8)> + TSet hash link(8).
const LAYER_ENTRY_STRIDE: usize = 0x18;
const OFF_LAYER_CONTAINER: usize = 0x08;
const MAX_UI_MAP_SLOTS: u32 = 4096;

// UObject / UField / UStruct / UClass / UFunction members.
const OFF_NAME: usize = 0x18;
const OFF_OUTER: usize = 0x20;
const OFF_FIELD_NEXT: usize = 0x28;
const OFF_SUPER_STRUCT: usize = 0x40;
const OFF_STRUCT_CHILDREN: usize = 0x48;
const OFF_CAST_FLAGS: usize = 0xD8;
const OFF_FUNCTION_FLAGS: usize = 0xB0;
const FUNC_NATIVE: u32 = 0x400;

static GOBJECTS: AtomicUsize = AtomicUsize::new(0);
static GNAMES: AtomicUsize = AtomicUsize::new(0);
// GameInstance outlives temporary World/camera links while independent UI screens are open.
static CUR_GAME_INSTANCE: AtomicUsize = AtomicUsize::new(0);
static CUR_UI_MANAGER: AtomicUsize = AtomicUsize::new(0);
static CUR_LAYOUT: AtomicUsize = AtomicUsize::new(0);
static UI_MANAGER_SCAN_INSTANCE: AtomicUsize = AtomicUsize::new(0);
static UI_MANAGER_SCAN_CURSOR: AtomicUsize = AtomicUsize::new(0);
// Address-change logging only; never used to resolve a possibly detached character UI.
static CUR_AWAKEN: AtomicUsize = AtomicUsize::new(0);
static SPINE_RESOLVE_TRIES: AtomicU64 = AtomicU64::new(0);
static SPINE_NEXT_SCAN: AtomicU64 = AtomicU64::new(0);

fn count(objects: usize) -> u32 {
    if objects == 0 {
        return 0;
    }
    match read_u32(objects + OFF_NUM_ELEMENTS) {
        Some(n) if n <= MAX_ELEMENTS => n,
        _ => 0,
    }
}

fn chunks_ptr(objects: usize) -> usize {
    if objects == 0 {
        return 0;
    }
    match read_u32(objects + OFF_NUM_CHUNKS) {
        Some(n) if n > 0 && n <= MAX_CHUNKS => {}
        _ => return 0,
    }
    read_u64(objects).unwrap_or(0) as usize
}

fn gobjects() -> usize {
    GOBJECTS.load(Ordering::Relaxed)
}

fn gnames() -> usize {
    GNAMES.load(Ordering::Relaxed)
}

fn chunk_at(chunks: usize, chunk_idx: u32) -> usize {
    if chunks == 0 {
        return 0;
    }
    read_u64(chunks + chunk_idx as usize * 8).unwrap_or(0) as usize
}

fn item_at(chunk: usize, index: u32) -> usize {
    if chunk == 0 {
        return 0;
    }
    read_u64(chunk + (index % ELEMENTS_PER_CHUNK) as usize * FUOBJECT_ITEM).unwrap_or(0) as usize
}

fn matches(src: &[u8], tgt: &[u8]) -> bool {
    if src.len() == tgt.len() {
        return src == tgt;
    }
    if src.len() > tgt.len() && src.len() - tgt.len() <= 2 {
        return &src[..tgt.len()] == tgt;
    }
    false
}

fn read_fname(pool: usize, id: u32, out: &mut [u8]) -> usize {
    let block = match read_u64(pool + POOL_BLOCKS + ((id >> BLOCK_SHIFT) as usize) * 8) {
        Some(v) if v != 0 => v as usize,
        _ => return 0,
    };
    let entry = block + (id & BLOCK_MASK) as usize * 2;
    let h = match read_u16(entry) {
        Some(v) => v,
        None => return 0,
    };
    // UE 5.6 FNameEntryHeader: bIsWide:1, LowercaseProbeHash:5, Len:10
    let mut len = (h >> 6) as usize;
    let wide = (h & 1) != 0;
    if len == 0 {
        return 0;
    }
    if len > MAX_NAME {
        len = MAX_NAME;
    }
    let limit = min(out.len(), len);
    let mut buf = [0u8; MAX_NAME * 2];
    let want = min(if wide { limit * 2 } else { limit }, buf.len());
    let got = unsafe { read_raw(entry + ENTRY_HEADER, buf.as_mut_ptr(), want) };
    let mut n = 0usize;
    if wide {
        let mut i = 0usize;
        while i + 1 < got && n < limit {
            if buf[i + 1] != 0 {
                break;
            }
            let c = buf[i];
            if !(0x20..0x7f).contains(&c) {
                break;
            }
            out[n] = c;
            n += 1;
            i += 2;
        }
    } else {
        while n < limit && n < got {
            let c = buf[n];
            if !(0x20..0x7f).contains(&c) {
                break;
            }
            out[n] = c;
            n += 1;
        }
    }
    n
}

fn read_ptr(address: usize) -> usize {
    read_u64(address).unwrap_or(0) as usize
}

fn heapish(module: &Module, p: u64) -> bool {
    p != 0 && p % 8 == 0 && p >= 0x1_0000 && !module.contains(p as usize)
}

fn check_objects(b: &[u8], i: usize) -> bool {
    let max = rd_u32(b, i + OFF_MAX_ELEMENTS);
    let num = rd_u32(b, i + OFF_NUM_ELEMENTS);
    let max_chunks = rd_u32(b, i + OFF_MAX_CHUNKS);
    let num_chunks = rd_u32(b, i + OFF_NUM_CHUNKS);
    if num <= UA_MIN_ELEMENTS || max <= UA_MIN_MAX_ELEMENTS || num > max {
        return false;
    }
    if num_chunks < UA_MIN_CHUNKS || num_chunks > UA_MAX_CHUNKS {
        return false;
    }
    if max_chunks < UA_MIN_MAX_CHUNKS || max_chunks > UA_MAX_MAX_CHUNKS || num_chunks > max_chunks {
        return false;
    }
    if max % UA_CHUNK_ALIGN != 0 {
        return false;
    }
    let per_chunk = max / max_chunks;
    if per_chunk % UA_CHUNK_ALIGN != 0
        || per_chunk < UA_MIN_PER_CHUNK
        || per_chunk > UA_MAX_PER_CHUNK
    {
        return false;
    }
    if max / per_chunk != max_chunks {
        return false;
    }
    (num / per_chunk) + 1 == num_chunks
}

fn objects_contain(a: usize, world: usize) -> bool {
    let num = read_u32(a + OFF_NUM_ELEMENTS).unwrap_or(0);
    let max = read_u32(a + OFF_MAX_ELEMENTS).unwrap_or(0);
    let max_chunks = read_u32(a + OFF_MAX_CHUNKS).unwrap_or(0);
    let num_chunks = read_u32(a + OFF_NUM_CHUNKS).unwrap_or(0);
    if max_chunks == 0 {
        return false;
    }
    let per_chunk = max / max_chunks;
    if per_chunk == 0 {
        return false;
    }
    let chunks = read_u64(a).unwrap_or(0) as usize;
    if chunks == 0 {
        return false;
    }
    let mut c = 0u32;
    while c < num_chunks {
        if read_u64(chunks + c as usize * 8).unwrap_or(0) == 0 {
            return false;
        }
        c += 1;
    }
    let mut i = 0u32;
    while i < num {
        let chunk = read_u64(chunks + (i / per_chunk) as usize * 8).unwrap_or(0) as usize;
        if chunk == 0 {
            return false;
        }
        let obj = read_u64(chunk + (i % per_chunk) as usize * FUOBJECT_ITEM).unwrap_or(0) as usize;
        if obj == world {
            return true;
        }
        i += 1;
    }
    false
}

fn discover_objects(module: &Module, world: usize) -> Option<usize> {
    scan(module, 0x20, |at, b, i| {
        check_objects(b, i) && objects_contain(at, world)
    })
}

fn names_world(pool: usize, world: usize) -> bool {
    let class = read_ptr(world + OFF_CLASS);
    if class == 0 {
        return false;
    }
    let id = read_u32(class + OFF_NAME).unwrap_or(0);
    let mut buf = [0u8; 8];
    let n = read_fname(pool, id, &mut buf);
    // Same two-character suffix tolerance the rest of the file uses, so a uniquified class name cannot disable the whole plugin.
    n >= 5 && n <= 7 && &buf[..5] == b"World"
}

fn discover_names(module: &Module, world: usize) -> Option<usize> {
    scan(module, 0x30, |at, b, i| {
        if rd_u32(b, i) != 0 {
            return false;
        }
        let cur_block = rd_u32(b, i + 0x08);
        let cur_cursor = rd_u32(b, i + 0x0C);
        if cur_block as usize >= FNAME_BLOCKS_MAX {
            return false;
        }
        if cur_cursor < 0x10 || cur_cursor as usize >= FNAME_BLOCK_BYTES_MAX {
            return false;
        }
        let mut k = 0usize;
        while k < 4 {
            if !heapish(module, rd_u64(b, i + POOL_BLOCKS + k * 8)) {
                return false;
            }
            k += 1;
        }
        names_world(at, world)
    })
}

/// Finds GObjects and GNames after the common GWorld scan, retrying a few ticks apart.
fn ensure_spine_globals(module: &Module) -> bool {
    if gobjects() != 0 && gnames() != 0 {
        return true;
    }
    let tries = SPINE_RESOLVE_TRIES.fetch_add(1, Ordering::Relaxed);
    if tries < SPINE_NEXT_SCAN.load(Ordering::Relaxed) {
        return false;
    }
    // wait 4 ticks for the next object/name scan
    SPINE_NEXT_SCAN.store(tries + 4, Ordering::Relaxed);

    let world = read_u64(WORLD_SLOT.load(Ordering::Relaxed)).unwrap_or(0) as usize;
    if world == 0 {
        return false;
    }
    if gobjects() == 0 {
        match discover_objects(module, world) {
            Some(a) => {
                GOBJECTS.store(a, Ordering::Relaxed);
                log_global("GObjects", a, module);
            }
            None => {
                if tries == 0 {
                    log::line("GObjects not found yet; the scan will retry");
                }
            }
        }
    }
    if gnames() == 0 {
        match discover_names(module, world) {
            Some(a) => {
                GNAMES.store(a, Ordering::Relaxed);
                log_global("GNames", a, module);
            }
            None => {
                if tries == 0 {
                    log::line("GNames not found yet; the scan will retry");
                }
            }
        }
    }
    gobjects() != 0 && gnames() != 0
}

fn log_global(label: &str, addr: usize, module: &Module) {
    let mut b = Buf::new();
    b.push_str(label);
    b.push_str(" rva 0x");
    b.push_hex(addr.wrapping_sub(module.base) as u64, 0);
    log::log_buf(&b);
}

fn named(address: usize, target: &str) -> bool {
    let Some(name) = read_u64(address) else {
        return false;
    };
    if name >> 32 != 0 {
        return false;
    }
    let mut buf = [0u8; MAX_NAME];
    let len = read_fname(gnames(), name as u32, &mut buf);
    len == target.len() && &buf[..len] == target.as_bytes()
}

fn class_chain_matches(module: &Module, pool: usize, class: usize, target: &[u8]) -> bool {
    let mut cur = class;
    let mut depth = 0usize;
    while cur != 0 && depth < MAX_SUPER_DEPTH {
        if !valid_object(module, cur) {
            return false;
        }
        let id = match read_u32(cur + OFF_NAME) {
            Some(v) => v,
            None => return false,
        };
        let mut buf = [0u8; MAX_NAME];
        let len = read_fname(pool, id, &mut buf);
        if len > 0 && matches(&buf[..len], target) {
            return true;
        }
        cur = read_ptr(cur + OFF_SUPER_STRUCT);
        depth += 1;
    }
    false
}

fn find_function(module: &Module, object: usize, name: &str) -> usize {
    let mut class = read_ptr(object + OFF_CLASS);
    let mut depth = 0usize;
    while class != 0 && depth < MAX_SUPER_DEPTH {
        if !valid_object(module, class) {
            break;
        }
        let mut field = read_ptr(class + OFF_STRUCT_CHILDREN);
        let mut fields = 0usize;
        while field != 0 && fields < 8192 {
            if !valid_object(module, field) {
                break;
            }
            let field_class = read_ptr(field + OFF_CLASS);
            let cast = read_u64(field_class + OFF_CAST_FLAGS).unwrap_or(0);
            if cast & 0x80000 != 0 && named(field + OFF_NAME, name) {
                return field;
            }
            field = read_ptr(field + OFF_FIELD_NEXT);
            fields += 1;
        }
        class = read_ptr(class + OFF_SUPER_STRUCT);
        depth += 1;
    }
    0
}

fn call_function(module: &Module, object: usize, name: &str, params: *mut c_void) -> bool {
    let Some(d) = describe(module, object) else {
        return false;
    };
    let function = find_function(module, object, name);
    if function == 0 {
        return false;
    }
    let entry = read_ptr(d.vtable + PROCESSEVENT_IDX * 8);
    let Some(flags) = read_u32(function + OFF_FUNCTION_FLAGS) else {
        return false;
    };
    if !module.contains(entry)
        || !write_bytes(function + OFF_FUNCTION_FLAGS, &(flags | FUNC_NATIVE).to_ne_bytes())
    {
        return false;
    }
    let invoke: unsafe extern "system" fn(usize, usize, *mut c_void) = unsafe { core::mem::transmute(entry) };
    // Local visual refresh on the plugin worker; no RPCs or hooks.
    unsafe { invoke(object, function, params) };
    write_bytes(function + OFF_FUNCTION_FLAGS, &flags.to_ne_bytes())
}

fn typed(module: &Module, object: usize, target: &[u8]) -> bool {
    let Some(d) = describe(module, object) else {
        return false;
    };
    d.flags & RF_CDO_OR_ARCHETYPE == 0
        && class_chain_matches(module, gnames(), read_ptr(object + OFF_CLASS), target)
}

fn current_game_instance(module: &Module) -> usize {
    let world = read_ptr(WORLD_SLOT.load(Ordering::Relaxed));
    if valid_object(module, world) {
        let instance = read_ptr(world + OFF_WORLD_GI);
        if typed(module, instance, GAME_INSTANCE_CLASS) {
            if CUR_GAME_INSTANCE.swap(instance, Ordering::Relaxed) != instance {
                log_object("spine game instance", instance);
            }
            return instance;
        }
    }
    let cached = CUR_GAME_INSTANCE.load(Ordering::Relaxed);
    if !typed(module, cached, GAME_INSTANCE_CLASS) {
        return 0;
    }
    cached
}

fn layout_matches(module: &Module, object: usize, instance: usize) -> bool {
    object != 0
        && instance != 0
        && read_ptr(object + OFF_OUTER) == instance
        && typed(module, object, OVERALL_LAYOUT_CLASS)
}

fn function_ui_stack(layout: usize) -> Option<usize> {
    let map = layout + OFF_LAYOUT_LAYERS;
    let data = read_ptr(map);
    let slots = read_u32(map + 0x08)?;
    let max = read_u32(map + 0x0C)?;
    let bits = read_u32(map + 0x28)?;
    let max_bits = read_u32(map + 0x2C)?;
    let free = read_u32(map + 0x34)?;
    let heap = read_ptr(map + 0x20);
    if slots > max
        || max > MAX_UI_MAP_SLOTS
        || free > slots
        || bits < slots
        || bits > max_bits
        || max_bits > MAX_UI_MAP_SLOTS
        || (slots != 0 && data == 0)
        || (heap == 0 && bits > 128)
    {
        return None;
    }
    let flags = if heap == 0 { map + 0x10 } else { heap };
    let mut index = 0u32;
    let mut word = 0u32;
    while index < slots {
        if index & 31 == 0 {
            word = read_u32(flags + (index / 32) as usize * 4)?;
        }
        if word & (1 << (index & 31)) != 0 {
            let entry = data + index as usize * LAYER_ENTRY_STRIDE;
            if named(entry, "Layer.FunctionUI") {
                return read_u64(entry + OFF_LAYER_CONTAINER).map(|v| v as usize);
            }
        }
        index += 1;
    }
    None
}

fn ui_manager_matches(module: &Module, object: usize, instance: usize) -> bool {
    object != 0
        && instance != 0
        && read_ptr(object + OFF_OUTER) == instance
        && typed(module, object, UI_MANAGER_CLASS)
}

fn resolve_ui_manager(module: &Module, instance: usize) -> usize {
    let current = CUR_UI_MANAGER.load(Ordering::Relaxed);
    if ui_manager_matches(module, current, instance) {
        return current;
    }
    CUR_UI_MANAGER.store(0, Ordering::Relaxed);
    let changed_instance = UI_MANAGER_SCAN_INSTANCE.swap(instance, Ordering::Relaxed) != instance;
    if current != 0 || changed_instance {
        UI_MANAGER_SCAN_CURSOR.store(0, Ordering::Relaxed);
    }
    let total = count(gobjects());
    let chunks = chunks_ptr(gobjects());
    if total == 0 || chunks == 0 {
        return 0;
    }
    let cursor = UI_MANAGER_SCAN_CURSOR.load(Ordering::Relaxed) as u32;
    let start = if cursor < total { cursor } else { 0 };
    let end = min(start.saturating_add(UI_MANAGER_SCAN_BATCH), total);
    let mut index = start;
    let mut chunk_idx = u32::MAX;
    let mut chunk = 0usize;
    while index < end {
        let cidx = index >> BLOCK_SHIFT;
        if cidx != chunk_idx {
            chunk = chunk_at(chunks, cidx);
            chunk_idx = cidx;
            if chunk == 0 {
                index = min((cidx + 1) << BLOCK_SHIFT, end);
                continue;
            }
        }
        let object = item_at(chunk, index);
        index += 1;
        if ui_manager_matches(module, object, instance) {
            CUR_UI_MANAGER.store(object, Ordering::Relaxed);
            log_object("spine UI manager", object);
            return object;
        }
    }
    UI_MANAGER_SCAN_CURSOR.store(if end == total { 0 } else { end as usize }, Ordering::Relaxed);
    0
}

fn resolve_layout(module: &Module) -> usize {
    let instance = current_game_instance(module);
    if instance == 0 {
        return 0;
    }
    let manager = resolve_ui_manager(module, instance);
    if manager == 0 {
        return 0;
    }
    let policy = read_ptr(manager + OFF_UI_MANAGER_POLICY);
    if !typed(module, policy, POLICY_CLASS) {
        return 0;
    }
    let Some((players, _)) = array(instance + OFF_GI_PLAYERS, 32) else {
        return 0;
    };
    let player = read_ptr(players);
    let Some((roots, count)) = array(policy + OFF_POLICY_ROOT_LAYOUTS, MAX_UI_ROOTS) else {
        return 0;
    };
    let mut i = 0u32;
    while i < count {
        let row = roots + i as usize * ROOT_LAYOUT_STRIDE;
        if read_ptr(row) == player && read_u8(row + OFF_ROOT_ADDED_TO_VIEWPORT) == Some(1) {
            let layout = read_ptr(row + OFF_ROOT_LAYOUT);
            if layout_matches(module, layout, instance) {
                if CUR_LAYOUT.swap(layout, Ordering::Relaxed) != layout {
                    log_object("spine registered layout", layout);
                }
                return layout;
            }
        }
        i += 1;
    }
    CUR_LAYOUT.store(0, Ordering::Relaxed);
    0
}

/// Empty FunctionUI stacks and uninitialized character pages are normal; never scan for Awaken.
fn function_ui_awaken(module: &Module, layout: usize) -> usize {
    let Some(stack) = function_ui_stack(layout) else {
        return 0;
    };
    if !typed(module, stack, CONTAINER_CLASS) {
        return 0;
    }
    let main = read_ptr(stack + OFF_CONTAINER_DISPLAYED);
    if !typed(module, main, CHARACTER_MAIN_CLASS)
        || read_u8(main + OFF_UI_CLOSING) == Some(1)
    {
        return 0;
    }
    let info = read_ptr(main + OFF_MAIN_CHARACTER_INFO);
    if !typed(module, info, CHARACTER_INFO_CLASS) {
        return 0;
    }
    let owner = read_ptr(info + OFF_INFO_AWAKEN);
    if !typed(module, owner, AWAKEN_CLASS) || read_u8(owner + OFF_UI_CLOSING) == Some(1) {
        return 0;
    }
    if awaken_spine(module, owner) == 0 {
        return 0;
    }
    owner
}

fn awaken_spine(module: &Module, owner: usize) -> usize {
    let Some(owner_desc) = describe(module, owner) else {
        return 0;
    };
    if owner_desc.flags & RF_CDO_OR_ARCHETYPE != 0
        || !class_chain_matches(module, gnames(), read_ptr(owner + OFF_CLASS), AWAKEN_CLASS)
    {
        return 0;
    }
    let spine = read_ptr(owner + OFF_AWAKEN_SPINE);
    let Some(spine_desc) = describe(module, spine) else {
        return 0;
    };
    if spine_desc.flags & RF_CDO_OR_ARCHETYPE != 0
        || !class_chain_matches(module, gnames(), read_ptr(spine + OFF_CLASS), AWAKEN_SPINE_CLASS)
    {
        return 0;
    }
    spine
}

fn resolve_awaken(module: &Module) -> usize {
    let layout = resolve_layout(module);
    let owner = if layout == 0 {
        0
    } else {
        function_ui_awaken(module, layout)
    };
    if CUR_AWAKEN.swap(owner, Ordering::Relaxed) != owner && owner != 0 {
        log_object("spine FunctionUI awaken", owner);
    }
    owner
}

fn set_visibility(module: &Module, widget: usize, visibility: u8) -> bool {
    if !valid_object(module, widget) {
        return false;
    }
    if read_u8(widget + OFF_WIDGET_VISIBILITY) == Some(visibility) {
        return true;
    }
    let mut params = visibility;
    call_function(
        module,
        widget,
        "SetVisibility",
        (&mut params as *mut u8).cast(),
    ) && read_u8(widget + OFF_WIDGET_VISIBILITY) == Some(visibility)
}

/// Presentation only: keep the already-packaged final Spine visible without changing awaken data.
fn patch_awaken(module: &Module) {
    let owner = resolve_awaken(module);
    if owner == 0 {
        return;
    }
    let spine = awaken_spine(module, owner);
    if spine == 0 {
        return;
    }
    let targets = [
        (OFF_AWAKEN_IMAGE_GRAY, COLLAPSED),
        (OFF_AWAKEN_IMAGE_NORMAL, COLLAPSED),
        (OFF_AWAKEN_SPINE_CHARACTER, VISIBLE),
        (OFF_AWAKEN_SPINE_EFFECT, COLLAPSED),
    ];
    let mut i = 0usize;
    while i < targets.len() {
        let (offset, visibility) = targets[i];
        let widget = read_ptr(spine + offset);
        if widget != 0 {
            set_visibility(module, widget, visibility);
        }
        i += 1;
    }
}
