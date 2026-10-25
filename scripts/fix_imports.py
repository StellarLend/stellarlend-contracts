with open('stellar-lend/contracts/lending/src/events.rs', 'r', encoding='utf-8') as f:
    ev = f.read()

# In events.rs, DiagnosticSeverity was separated from DiagnosticsEvent when we put #[cfg(test)]
# Let's see: DiagnosticSeverity was gated, but DiagnosticsEvent wasn't or vice versa
# Let's gate the entire diagnostics block (from DiagnosticSeverity down to the end of events.rs)
ev = ev.replace('#[cfg(test)]\n/// Severity level', '/// Severity level')
idx = ev.find('pub enum DiagnosticSeverity')
if idx != -1:
    doc_idx = ev.rfind('/// Severity level', 0, idx)
    if doc_idx != -1:
        ev = ev[:doc_idx] + '#[cfg(test)]\n' + ev[doc_idx:]

with open('stellar-lend/contracts/lending/src/events.rs', 'w', encoding='utf-8') as f:
    f.write(ev)

# In lib.rs, add panic_with_error to soroban_sdk imports
with open('stellar-lend/contracts/lending/src/lib.rs', 'r', encoding='utf-8') as f:
    lib = f.read()

lib = lib.replace('use soroban_sdk::{', 'use soroban_sdk::{panic_with_error, ')

with open('stellar-lend/contracts/lending/src/lib.rs', 'w', encoding='utf-8') as f:
    f.write(lib)

print("Fixed imports and events.rs gating")
