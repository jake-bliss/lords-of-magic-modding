import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.data.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;

import java.io.*;
import java.nio.file.*;
import java.util.*;

public class ImportKnown extends GhidraScript {
    private static final CategoryPath CAT = new CategoryPath("/LOM");

    private Path repo;
    private int opRenamed, opCreated, opAliased, opSame;
    private int structsNested, structsMade, structsApplied, structsSame, ptrsApplied;
    private int labels, labelsSame, knownFuncs, conflicts;
    private final List<String> conflictLog = new ArrayList<>();

    @Override
    public void run() throws Exception {
        repo = Paths.get(getScriptArgs()[0]);
        int tx = currentProgram.startTransaction("ImportKnown");
        try {
            importOperators();
            if (opCreated > 0) {
                int same = opSame;
                importOperators();
                opSame = same;
            }
            importStructures();
            importClusters();
            importDirectionTables();
            importKnownSymbols();
        } finally {
            currentProgram.endTransaction(tx, true);
        }
        for (String c : conflictLog) println("CONFLICT " + c);
        println("SUMMARY operators renamed=" + opRenamed + " created=" + opCreated + " aliased=" + opAliased
            + " unchanged=" + opSame + " | structs made=" + structsMade + " applied=" + structsApplied
            + " nested=" + structsNested + " pointers=" + ptrsApplied + " unchanged=" + structsSame + " | labels=" + labels
            + " unchanged=" + labelsSame + " | known functions=" + knownFuncs + " | conflicts skipped=" + conflicts);
    }

    private List<Map<String, String>> readTsv(String rel) throws IOException {
        List<String> lines = Files.readAllLines(repo.resolve(rel));
        String[] head = lines.get(0).split("\t", -1);
        List<Map<String, String>> out = new ArrayList<>();
        for (int i = 1; i < lines.size(); i++) {
            if (lines.get(i).isBlank()) continue;
            String[] c = lines.get(i).split("\t", -1);
            Map<String, String> m = new LinkedHashMap<>();
            for (int j = 0; j < head.length && j < c.length; j++) m.put(head[j], c[j]);
            out.add(m);
        }
        return out;
    }

    private Address addr(String hex) {
        long v = Long.parseUnsignedLong(hex.replaceFirst("^0[xX]", ""), 16);
        return currentProgram.getAddressFactory().getDefaultAddressSpace().getAddress(v);
    }

    private void conflict(String what) {
        conflicts++;
        conflictLog.add(what);
    }

    private boolean isDefaultName(Symbol s) {
        if (s.getSource() == SourceType.DEFAULT) return true;
        String n = s.getName();
        return n.startsWith("FUN_") || n.startsWith("DAT_") || n.startsWith("PTR_") || n.startsWith("LAB_")
            || n.startsWith("thunk_FUN_") || n.startsWith("UNK_");
    }

    static String opIdent(String name) {
        StringBuilder sb = new StringBuilder("op_");
        for (char ch : name.toCharArray()) {
            if (ch == '?') sb.append("_p");
            else if (Character.isLetterOrDigit(ch) && ch < 128 || ch == '_') sb.append(ch);
            else sb.append('_');
        }
        return sb.toString();
    }

    private void importOperators() throws Exception {
        Map<String, String> mapping = new TreeMap<>();
        Set<Address> seen = new HashSet<>();
        FunctionManager fm = currentProgram.getFunctionManager();
        for (Map<String, String> r : readTsv("reports/natives/operator-bodies.tsv")) {
            String name = r.get("name");
            String ident = opIdent(name);
            if (!ident.equals("op_" + name)) mapping.put(name, ident);
            Address a = addr(r.get("entry_point"));
            if (!currentProgram.getMemory().contains(a)) { conflict("operator " + name + " " + a + " outside memory"); continue; }
            Function f = fm.getFunctionAt(a);
            if (f == null) {
                if (currentProgram.getListing().getInstructionAt(a) == null) {
                    new DisassembleCommand(a, null, true).applyTo(currentProgram, monitor);
                }
                f = createFunction(a, ident);
                if (f == null) { conflict("operator " + name + " " + a + " could not create function"); continue; }
                opCreated++;
                seen.add(a);
                continue;
            }
            if (!seen.add(a)) {
                if (hasLabel(a, ident)) opSame++;
                else {
                    currentProgram.getSymbolTable().createLabel(a, ident, SourceType.USER_DEFINED);
                    opAliased++;
                }
                continue;
            }
            if (f.getName().equals(ident)) { opSame++; continue; }
            if (!isDefaultName(f.getSymbol()) && !f.getName().startsWith("op_")) {
                conflict("operator " + name + " " + a + " already named " + f.getName());
                continue;
            }
            f.setName(ident, SourceType.USER_DEFINED);
            opRenamed++;
        }
        StringBuilder sb = new StringBuilder("name\tidentifier\n");
        mapping.forEach((k, v) -> sb.append(k).append('\t').append(v).append('\n'));
        Files.writeString(repo.resolve("tools/ghidra/operator-name-map.tsv"), sb.toString());
    }

    private boolean hasLabel(Address a, String name) {
        for (Symbol s : currentProgram.getSymbolTable().getSymbols(a)) if (s.getName().equals(name)) return true;
        return false;
    }

    private void label(Address a, String name, String why) throws Exception {
        if (!currentProgram.getMemory().contains(a)) { conflict(why + " " + a + " outside memory"); return; }
        if (hasLabel(a, name)) { labelsSame++; return; }
        Symbol p = currentProgram.getSymbolTable().getPrimarySymbol(a);
        boolean primary = p == null || isDefaultName(p) || p.getName().startsWith("g_") || p.getName().startsWith("gp_");
        currentProgram.getSymbolTable().createLabel(a, name, currentProgram.getGlobalNamespace(), SourceType.USER_DEFINED);
        if (primary) {
            Symbol s = currentProgram.getSymbolTable().getGlobalSymbol(name, a);
            if (s != null) s.setPrimary();
        }
        labels++;
    }

    private DataType intType(int w) {
        switch (w) {
            case 1: return ByteDataType.dataType;
            case 2: return ShortDataType.dataType;
            case 8: return LongLongDataType.dataType;
            default: return IntegerDataType.dataType;
        }
    }

    private static int maxWidth(String widths) {
        int m = 0;
        for (String p : widths.split("\\|")) m = Math.max(m, Integer.parseInt(p));
        return m;
    }

    private static String cap(String s) {
        return s.isEmpty() ? s : Character.toUpperCase(s.charAt(0)) + s.substring(1);
    }

    private boolean rangeFree(Address a, int len) {
        Listing l = currentProgram.getListing();
        Address end;
        try { end = a.addNoWrap(len - 1); } catch (AddressOverflowException e) { return false; }
        if (!currentProgram.getMemory().contains(a) || !currentProgram.getMemory().contains(end)) return false;
        if (l.getInstructions(new AddressSet(a, end), true).hasNext()) return false;
        Data cont = l.getDataContaining(a);
        if (cont != null && cont.getDataType().getCategoryPath().toString().startsWith("/LOM")) return false;
        DataIterator it = l.getDefinedData(new AddressSet(a, end), true);
        while (it.hasNext()) {
            DataType t = it.next().getDataType();
            if (t.getCategoryPath().toString().startsWith("/LOM")) return false;
            if (t instanceof Composite || t instanceof Array || t instanceof AbstractStringDataType) return false;
        }
        return true;
    }

    private void place(Address a, DataType dt) throws Exception {
        currentProgram.getListing().clearCodeUnits(a, a.add(dt.getLength() - 1), false);
        currentProgram.getListing().createData(a, dt);
    }

    private static final class Built {
        String key, tname, gname, kind;
        Address base;
        DataType dt, placed;
        long lo, hi;
        boolean nested;
    }

    private void importStructures() throws Exception {
        List<Map<String, String>> structs = readTsv("reports/natives/state/structures.tsv");
        List<Map<String, String>> fields = readTsv("reports/natives/state/structure-fields.tsv");
        Map<String, List<Map<String, String>>> byKey = new HashMap<>();
        for (Map<String, String> f : fields) byKey.computeIfAbsent(f.get("base") + "/" + f.get("base_kind"), k -> new ArrayList<>()).add(f);
        structs.sort(Comparator.comparingLong(s -> Long.decode(s.get("observed_extent"))));
        DataTypeManager dtm = currentProgram.getDataTypeManager();
        List<Built> built = new ArrayList<>();
        for (Map<String, String> s : structs) {
            Built b = new Built();
            b.kind = s.get("base_kind");
            b.base = addr(s.get("base"));
            String hex = Long.toHexString(b.base.getOffset());
            String ev = s.get("name_evidence");
            String top = ev.equals("-") || ev.isEmpty() ? "Object" : cap(ev.split(",")[0].split(":")[0]) + "State";
            b.tname = top + "_" + hex + (b.kind.equals("indirect") ? "_ind" : "");
            b.gname = (b.kind.equals("indirect") ? "gp_" : "g_") + top + "_" + hex;
            int extent = (int) Long.decode(s.get("observed_extent")).longValue();
            List<Map<String, String>> fs = new ArrayList<>(byKey.getOrDefault(s.get("base") + "/" + b.kind, List.of()));
            fs.sort(Comparator.comparingLong(f -> Long.decode(f.get("offset"))));

            StructureDataType st = new StructureDataType(CAT, b.tname, Math.max(extent, 1), dtm);
            int end = 0;
            for (Map<String, String> f : fs) {
                int off = (int) Long.decode(f.get("offset")).longValue();
                int w = Math.max(1, maxWidth(f.get("widths")));
                if (off < end) continue;
                if (off + w > st.getLength()) st.growStructure(off + w - st.getLength());
                st.replaceAtOffset(off, intType(w), w, String.format("f_%02x", off), null);
                end = off + w;
            }
            long lo = b.base.getOffset();
            long hi = lo + st.getLength();
            List<Built> inner = new ArrayList<>();
            for (Built c : built) {
                if (c.kind.equals("static") && c.lo > lo && c.hi <= hi) inner.add(c);
                else if (c.kind.equals("indirect") && c.lo > lo && c.lo + 4 <= hi) inner.add(c);
            }
            inner.sort(Comparator.comparingLong((Built c) -> c.placed.getLength()).reversed());
            List<long[]> taken = new ArrayList<>();
            for (Built c : inner) {
                long clo = c.lo, chi = c.kind.equals("static") ? c.hi : c.lo + 4;
                boolean clash = false;
                for (long[] t : taken) if (clo < t[1] && t[0] < chi) { clash = true; break; }
                if (clash) continue;
                taken.add(new long[] {clo, chi});
                c.nested = true;
                int off = (int) (clo - lo);
                for (DataTypeComponent dc : st.getDefinedComponents()) {
                    if (dc.getOffset() < (int) (chi - lo) && dc.getOffset() + dc.getLength() > off) st.clearAtOffset(dc.getOffset());
                }
                st.replaceAtOffset(off, c.placed, (int) (chi - clo), String.format("n_%02x", off), null);
            }
            b.dt = dtm.resolve(st, DataTypeConflictHandler.REPLACE_HANDLER);
            b.placed = b.kind.equals("indirect") ? new PointerDataType(b.dt, 4, dtm) : b.dt;
            b.lo = lo;
            b.hi = hi;
            structsMade++;
            built.add(b);
        }
        built.sort(Comparator.comparingLong((Built b) -> b.placed.getLength()).reversed());
        for (Built b : built) {
            label(b.base, b.gname, "struct " + b.tname);
            if (b.nested) { structsNested++; continue; }
            int len = b.placed.getLength();
            Data d = currentProgram.getListing().getDataAt(b.base);
            if (d != null && (b.kind.equals("static") ? d.getDataType().getName().equals(b.tname) : d.getDataType().isEquivalent(b.placed))) { structsSame++; continue; }
            if (!rangeFree(b.base, len)) { conflict("struct " + b.tname + " at " + b.base + " size 0x" + Integer.toHexString(len) + " overlaps defined data"); continue; }
            place(b.base, b.placed);
            if (b.kind.equals("indirect")) ptrsApplied++; else structsApplied++;
        }
    }

    private void importClusters() throws Exception {
        for (Map<String, String> r : readTsv("reports/natives/global-clusters.tsv")) {
            Address a = addr(r.get("start"));
            label(a, "gcl_" + Long.toHexString(a.getOffset()), "cluster");
        }
    }

    private void importDirectionTables() throws Exception {
        for (Map<String, String> r : readTsv("reports/natives/direction-table.tsv")) {
            Address a = addr(r.get("va"));
            label(a, "dir_table_" + r.get("operand") + "_" + Long.toHexString(a.getOffset()), "direction table");
            ArrayDataType arr = new ArrayDataType(IntegerDataType.dataType, 8, 4);
            Data d = currentProgram.getListing().getDataAt(a);
            if (d != null && d.getDataType().isEquivalent(arr)) continue;
            if (!rangeFree(a, 32)) { conflict("direction table " + a + " overlaps defined data"); continue; }
            place(a, arr);
        }
    }

    private void importKnownSymbols() throws Exception {
        FunctionManager fm = currentProgram.getFunctionManager();
        SymbolTable st = currentProgram.getSymbolTable();
        for (Map<String, String> r : readTsv("tools/ghidra/known-symbols.tsv")) {
            Address a = addr(r.get("address"));
            String full = r.get("name");
            if (r.get("kind").equals("data")) { label(a, full, "known data"); continue; }
            Namespace ns = currentProgram.getGlobalNamespace();
            String leaf = full;
            int i = full.lastIndexOf("::");
            if (i >= 0) {
                ns = st.getOrCreateNameSpace(ns, full.substring(0, i), SourceType.USER_DEFINED);
                leaf = full.substring(i + 2);
            }
            Function f = fm.getFunctionAt(a);
            if (f == null) {
                if (currentProgram.getListing().getInstructionAt(a) == null) new DisassembleCommand(a, null, true).applyTo(currentProgram, monitor);
                f = createFunction(a, leaf);
                if (f == null) { conflict("known function " + full + " " + a + " could not create"); continue; }
            }
            if (f.getName().equals(leaf) && f.getParentNamespace().equals(ns)) { labelsSame++; continue; }
            String cur = f.getName();
            if (!isDefaultName(f.getSymbol()) && !cur.startsWith("op_") && !cur.equals(leaf)) {
                conflict("known function " + full + " " + a + " already named " + cur);
                continue;
            }
            if (cur.startsWith("op_")) { conflict("known function " + full + " " + a + " is operator " + cur); continue; }
            f.setName(leaf, SourceType.USER_DEFINED);
            f.setParentNamespace(ns);
            knownFuncs++;
        }
    }
}
