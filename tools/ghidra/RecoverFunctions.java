import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.cmd.function.CreateFunctionCmd;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.mem.*;
import ghidra.program.model.symbol.*;

import java.io.*;
import java.nio.file.*;
import java.util.*;

public class RecoverFunctions extends GhidraScript {
    private Memory mem;
    private MemoryBlock text;
    private Listing listing;
    private final Map<String, Integer> made = new TreeMap<>();
    private static final int MAX_FRAGMENT = 24;
    private int maxFragment;
    private int failed, fragments, fragmentBytes, fragmentsSkipped, repaired;

    @Override
    public void run() throws Exception {
        Path repo = Paths.get(getScriptArgs()[0]);
        mem = currentProgram.getMemory();
        listing = currentProgram.getListing();
        text = mem.getBlock(".text");
        int tx = currentProgram.startTransaction("RecoverFunctions");
        try {
            for (long ep : operatorEntries(repo)) tryMake(toAddr(ep), "operator_table");
            dataPointers();
            callTargets();
            int before;
            do {
                before = total();
                codeOperandReferences();
                callTargets();
                alignedAfterPadding();
                callTargets();
            } while (total() != before);
            repairFragmentedFunctions();
            attachFragments();
        } finally {
            currentProgram.endTransaction(tx, true);
        }
        println("SUMMARY created=" + total() + " " + made + " failed=" + failed + " repaired=" + repaired
            + " fragments=" + fragments + " fragmentBytes=" + fragmentBytes + " maxFragment=" + maxFragment + " fragmentsSkipped=" + fragmentsSkipped);
    }

    private int total() {
        int t = 0;
        for (int v : made.values()) t += v;
        return t;
    }

    private List<Long> operatorEntries(Path repo) throws IOException {
        List<String> lines = Files.readAllLines(repo.resolve("reports/natives/operator-bodies.tsv"));
        List<String> head = Arrays.asList(lines.get(0).split("\t", -1));
        int col = head.indexOf("entry_point");
        Set<Long> out = new TreeSet<>();
        for (int i = 1; i < lines.size(); i++) {
            if (lines.get(i).isBlank()) continue;
            out.add(Long.parseUnsignedLong(lines.get(i).split("\t", -1)[col].replaceFirst("^0[xX]", ""), 16));
        }
        return new ArrayList<>(out);
    }

    private void clearSet(AddressSetView set) {
        for (AddressRange r : set.getAddressRanges()) listing.clearCodeUnits(r.getMinAddress(), r.getMaxAddress(), false);
    }

    private boolean tryMake(Address a, String method) {
        return tryMake(a, method, 0);
    }

    private boolean tryMake(Address a, String method, int minBytes) {
        if (!text.contains(a) || getFunctionContaining(a) != null) return false;
        AddressSetView disassembled = null;
        if (listing.getInstructionAt(a) == null) {
            DisassembleCommand cmd = new DisassembleCommand(a, null, true);
            boolean ok;
            try {
                ok = cmd.applyTo(currentProgram, monitor);
            } catch (Exception e) {
                throw new RuntimeException(e);
            }
            if (!ok) { failed++; return false; }
            disassembled = cmd.getDisassembledAddressSet();
        }
        Function f = createFunction(a, null);
        if (f == null) {
            failed++;
            if (disassembled != null) clearSet(disassembled);
            return false;
        }
        if (f.getBody().getNumAddresses() < minBytes) {
            AddressSet body = new AddressSet(f.getBody());
            removeFunction(f);
            if (disassembled != null) clearSet(body);
            return false;
        }
        made.merge(method, 1, Integer::sum);
        return true;
    }

    private int byteAt(Address a) throws MemoryAccessException {
        return mem.getByte(a) & 0xff;
    }

    private boolean isPadding(Address a) {
        try {
            int b = byteAt(a);
            return b == 0x90 || b == 0xCC;
        } catch (MemoryAccessException e) {
            return false;
        }
    }

    private boolean followsPaddingOrRet(Address a) {
        try {
            int b = byteAt(a.subtract(1));
            return b == 0x90 || b == 0xCC || b == 0xC3 || byteAt(a.subtract(3)) == 0xC2;
        } catch (MemoryAccessException e) {
            return false;
        }
    }

    private boolean inCodeOrData(Address a) {
        if (getFunctionContaining(a) != null) return true;
        Data d = listing.getDefinedDataContaining(a);
        return d != null;
    }

    private void dataPointers() throws Exception {
        Address lo = text.getStart(), hi = text.getEnd();
        for (String name : new String[] {".rdata", ".data"}) {
            MemoryBlock b = mem.getBlock(name);
            if (b == null || !b.isInitialized()) continue;
            Set<Address> seen = new TreeSet<>();
            for (Address p = b.getStart(); p.compareTo(b.getEnd().subtract(3)) <= 0; p = p.add(4)) {
                long v = mem.getInt(p) & 0xffffffffL;
                Address t = toAddr(v);
                if (t.compareTo(lo) < 0 || t.compareTo(hi) > 0) continue;
                seen.add(t);
            }
            for (Address t : seen) {
                if (inCodeOrData(t) || isPadding(t)) continue;
                if (followsPaddingOrRet(t)) tryMake(t, "data_ptr_after_padding");
                else if ((t.getOffset() & 0xf) == 0) tryMake(t, "data_ptr_aligned16");
            }
        }
    }

    private boolean tablelike(Address a) {
        try {
            return pointerPairAt(a);
        } catch (MemoryAccessException e) {
            return false;
        }
    }

    private void codeOperandReferences() {
        Set<Address> targets = new TreeSet<>();
        for (Instruction i : listing.getInstructions(new AddressSet(text.getStart(), text.getEnd()), true)) {
            for (Reference r : i.getReferencesFrom()) {
                if (r.getReferenceType().isFlow() || i.getFlowType().isComputed()) continue;
                Address to = r.getToAddress();
                if (!text.contains(to) || getFunctionContaining(to) != null || tablelike(to)) continue;
                Data d = getDataAt(to);
                if (d != null && d.isDefined()) continue;
                targets.add(to);
            }
        }
        for (Address a : targets) tryMake(a, "code_operand");
    }

    private void callTargets() {
        for (int round = 0; round < 20; round++) {
            Set<Address> calls = new TreeSet<>(), jumps = new TreeSet<>();
            for (Instruction i : listing.getInstructions(new AddressSet(text.getStart(), text.getEnd()), true)) {
                FlowType ft = i.getFlowType();
                boolean call = ft.isCall();
                boolean jump = ft.isJump() && !ft.isConditional() && !ft.isComputed();
                if (!call && !jump) continue;
                if (getFunctionContaining(i.getAddress()) == null) continue;
                for (Reference r : i.getReferencesFrom()) {
                    Address to = r.getToAddress();
                    if (!text.contains(to) || getFunctionContaining(to) != null) continue;
                    if (call) calls.add(to);
                    else jumps.add(to);
                }
            }
            int before = total();
            for (Address a : calls) tryMake(a, "call_target");
            for (Address a : jumps) tryMake(a, "tail_jump_target");
            if (total() == before) break;
        }
    }

    private void alignedAfterPadding() throws Exception {
        List<Address> cands = new ArrayList<>();
        Address end = text.getEnd();
        for (Address a = text.getStart().add(16); a.compareTo(end) < 0; a = a.add(16)) {
            if (inCodeOrData(a) || !isPadding(a.subtract(1)) || isPadding(a)) continue;
            if (listing.getInstructionAt(a) != null) continue;
            cands.add(a);
        }
        for (Address a : cands) tryMake(a, "aligned_after_padding", 16);
    }

    private boolean isUnowned(Address a) {
        return listing.getInstructionAt(a) != null && getFunctionContaining(a) == null;
    }

    private boolean pointerPairAt(Address a) throws MemoryAccessException {
        if ((a.getOffset() & 3) != 0) return false;
        Address lo = text.getStart(), hi = text.getEnd();
        for (int k = 0; k < 2; k++) {
            Address q = a.add(4L * k);
            if (!mem.contains(q.add(3))) return false;
            Address t = toAddr(mem.getInt(q) & 0xffffffffL);
            if (t.compareTo(lo) < 0 || t.compareTo(hi) > 0) return false;
        }
        return true;
    }

    private boolean isUndefinedUnowned(Address a) {
        return text.contains(a) && listing.getInstructionContaining(a) == null
            && listing.getDefinedDataContaining(a) == null && getFunctionContaining(a) == null;
    }

    private void attachFragments() throws Exception {
        List<Address> ends = new ArrayList<>();
        for (Function f : currentProgram.getFunctionManager().getFunctions(text.getStart(), true)) {
            if (!text.contains(f.getEntryPoint())) break;
            for (AddressRange r : f.getBody().getAddressRanges()) ends.add(r.getMaxAddress());
        }
        for (Address e : ends) {
            Function pf = getFunctionContaining(e);
            Address a = e.add(1);
            if (pf == null || !isUndefinedUnowned(a)) continue;
            int b = byteAt(a);
            int run = 0;
            while (isUndefinedUnowned(a.add(run)) && byteAt(a.add(run)) == b) run++;
            if (b == 0xCC || (b == 0x90 && run >= 2) || (b == 0 && run >= 4)) continue;
            int n = 1;
            if (pointerPairAt(a)) continue;
            while (isUndefinedUnowned(a.add(n))) {
                Address q = a.add(n);
                int c = byteAt(q);
                if (c == 0xCC || pointerPairAt(q)) break;
                if (c == 0 && mem.contains(q.add(3)) && mem.getInt(q) == 0) break;
                n++;
            }
            if (n > MAX_FRAGMENT) { fragmentsSkipped++; continue; }
            Address last = a.add(n - 1);
            if (listing.getInstructionAt(a) == null) new DisassembleCommand(a, new AddressSet(a, last), true).applyTo(currentProgram, monitor);
            Address c = a;
            boolean ok = true;
            while (c.compareTo(last) <= 0) {
                Instruction i = listing.getInstructionAt(c);
                if (i == null) { ok = false; break; }
                c = c.add(i.getLength());
            }
            if (!ok || !c.equals(last.add(1))) { fragmentsSkipped++; continue; }
            AddressSet nb = new AddressSet(pf.getBody());
            nb.add(a, last);
            try {
                pf.setBody(nb);
                fragments++;
                fragmentBytes += n;
                maxFragment = Math.max(maxFragment, n);
            } catch (Exception ex) {
                fragmentsSkipped++;
            }
        }
    }

    private void repairFragmentedFunctions() throws Exception {
        List<Function> bad = new ArrayList<>();
        for (Function f : currentProgram.getFunctionManager().getFunctions(text.getStart(), true)) {
            if (!text.contains(f.getEntryPoint())) break;
            Instruction last = listing.getInstructionContaining(f.getBody().getMaxAddress());
            if (last == null || !last.hasFallthrough() || last.getFallThrough() == null) continue;
            Address x = last.getFallThrough();
            if (!text.contains(x) || listing.getInstructionContaining(x) != null || listing.getDefinedDataContaining(x) != null) continue;
            Instruction n = listing.getInstructionAfter(x);
            if (n == null || n.getAddress().subtract(x) > 15 || getFunctionContaining(n.getAddress()) != null) continue;
            bad.add(f);
        }
        for (Function f : bad) {
            Instruction last = listing.getInstructionContaining(f.getBody().getMaxAddress());
            Address x = last.getFallThrough();
            for (int step = 0; step < 1000 && x != null; step++) {
                if (listing.getInstructionAt(x) == null) {
                    Instruction n = listing.getInstructionAfter(x);
                    if (n == null || n.getAddress().subtract(x) > 15 || getFunctionContaining(n.getAddress()) != null) break;
                    clearListing(n.getMinAddress(), n.getMaxAddress());
                }
                disassemble(x);
                Instruction cur = listing.getInstructionAt(x);
                if (cur == null) break;
                while (true) {
                    Instruction nx = listing.getInstructionAt(cur.getMaxAddress().add(1));
                    if (nx == null || (getFunctionContaining(nx.getAddress()) != null && !f.equals(getFunctionContaining(nx.getAddress())))) break;
                    cur = nx;
                }
                x = cur.hasFallthrough() ? cur.getFallThrough() : null;
                if (x != null && (getFunctionContaining(x) != null || listing.getInstructionAt(x) != null)) x = null;
            }
            if (CreateFunctionCmd.fixupFunctionBody(currentProgram, f, monitor)) repaired++;
        }
    }
}
