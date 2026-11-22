#!/usr/bin/env python3
import sys
import os

def read_leb128(b, pos):
    res = 0
    shift = 0
    while True:
        byte = b[pos]
        pos += 1
        res |= (byte & 0x7F) << shift
        if (byte & 0x80) == 0:
            break
        shift += 7
    return res, pos

def analyze_wasm(path):
    with open(path, "rb") as f:
        data = f.read()

    pos = 8
    section_names = {
        0: "Custom", 1: "Type", 2: "Import", 3: "Function",
        4: "Table", 5: "Memory", 6: "Global", 7: "Export",
        8: "Start", 9: "Element", 10: "Code", 11: "Data", 12: "DataCount"
    }

    sections = []
    custom_sections = []
    code_size = 0
    exports = []

    while pos < len(data):
        sec_id = data[pos]
        pos += 1
        sec_len, pos = read_leb128(data, pos)
        sec_start = pos
        name = section_names.get(sec_id, f"Unknown({sec_id})")
        if sec_id == 0:
            name_len, p2 = read_leb128(data, sec_start)
            cname = data[p2:p2+name_len].decode("utf-8", errors="replace")
            custom_sections.append((cname, sec_len))
            sections.append((f"Custom ({cname})", sec_len))
        elif sec_id == 10:
            code_size = sec_len
            sections.append((name, sec_len))
        elif sec_id == 7:
            # Parse exports
            p = sec_start
            num_exports, p = read_leb128(data, p)
            for _ in range(num_exports):
                n_len, p = read_leb128(data, p)
                exp_name = data[p:p+n_len].decode("utf-8", errors="replace")
                p += n_len
                kind = data[p]
                p += 1
                idx, p = read_leb128(data, p)
                if kind == 0: # function export
                    exports.append((exp_name, idx))
            sections.append((name, sec_len))
        else:
            sections.append((name, sec_len))
        pos = sec_start + sec_len

    total = len(data)
    print(f"File: {path}")
    print(f"Total Size: {total} bytes ({total / 1024:.2f} KiB)")
    print("=" * 60)
    print(f"{'Section':<35} {'Size (B)':>10} {'Percentage':>12}")
    print("-" * 60)
    for sname, slen in sections:
        pct = (slen / total) * 100
        print(f"{sname:<35} {slen:>10} {pct:>11.2f}%")
    print("=" * 60)
    print(f"Total exported functions: {len(exports)}")

    # Function sizes in Code section
    for sname, slen in sections:
        if sname == "Code":
            sec_idx = data.find(bytes([10])) # we can find code section offset precisely
            # But let's find the exact sec_start for section 10
            break

    # Let's do exact parsing of code section:
    p = 8
    code_funcs = []
    num_imported_funcs = 0
    while p < len(data):
        sid = data[p]
        p += 1
        slen, p = read_leb128(data, p)
        sstart = p
        if sid == 2: # Import section
            ip = sstart
            n_imp, ip = read_leb128(data, ip)
            for _ in range(n_imp):
                m_len, ip = read_leb128(data, ip)
                ip += m_len
                f_len, ip = read_leb128(data, ip)
                ip += f_len
                kind = data[ip]
                ip += 1
                if kind == 0:
                    num_imported_funcs += 1
                elif kind in (1, 2, 3):
                    pass
                t_idx, ip = read_leb128(data, ip)
        elif sid == 10: # Code section
            cp = sstart
            num_funcs, cp = read_leb128(data, cp)
            for i in range(num_funcs):
                fsize, cp = read_leb128(data, cp)
                code_funcs.append((num_imported_funcs + i, fsize))
                cp += fsize
        p = sstart + slen

    export_map = {idx: name for name, idx in exports}
    code_funcs.sort(key=lambda x: x[1], reverse=True)

    print("\nTop 20 Largest Functions in Code Section:")
    print(f"{'Func Index':<12} {'Size (B)':>10} {'Exported Name'}")
    print("-" * 50)
    for fidx, fsize in code_funcs[:20]:
        ename = export_map.get(fidx, "<internal>")
        print(f"{fidx:<12} {fsize:>10} {ename}")

    # Inspect contractspecv0
    for cname, csize in custom_sections:
        if cname == "contractspecv0":
            sec_idx = data.find(b"contractspecv0")
            sec_bytes = data[sec_idx + len("contractspecv0"): sec_idx + csize]
            import re
            idents = re.findall(rb"[A-Za-z_][A-Za-z0-9_]{3,}", sec_bytes)
            unique_idents = sorted(set(i.decode("latin1") for i in idents))
            print(f"\nSpec section size: {csize} bytes, unique identifiers: {len(unique_idents)}")
            suspicious = [i for i in unique_idents if any(x in i for x in [
                "Operation", "Tracker", "TwoPhase", "Prepared", "Validation",
                "Auth", "FlashLoanState", "Audit", "State"
            ])]
            print(f"Spec types from unused modules ({len(suspicious)}): {suspicious[:30]}")

    # Inspect Data section
    for sname, slen in sections:
        if sname == "Data":
            # Data section strings
            import re
            p = 8
            while p < len(data):
                sid = data[p]
                p += 1
                sl, p = read_leb128(data, p)
                if sid == 11:
                    dbytes = data[p:p+sl]
                    strs = re.findall(rb"[ -~]{5,}", dbytes)
                    print(f"\nData section strings ({len(strs)} found):")
                    for s in strs[:25]:
                        print("  ", s.decode("latin1"))
                    break
                p += sl

    return total, sections, exports

if __name__ == "__main__":
    wasm_path = sys.argv[1] if len(sys.argv) > 1 else "stellar-lend/target/wasm32v1-none/release/stellarlend_lending.wasm"
    analyze_wasm(wasm_path)
