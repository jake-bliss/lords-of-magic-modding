import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import java.io.*;

public class DumpAll extends GhidraScript {
    @Override
    public void run() throws Exception {
        String out = getScriptArgs()[0];
        DecompInterface d = new DecompInterface();
        d.openProgram(currentProgram);
        int ok = 0, bad = 0, thunks = 0, lines = 0;
        try (PrintWriter c = new PrintWriter(new FileWriter(out + "/all.c"));
             PrintWriter idx = new PrintWriter(new FileWriter(out + "/functions.tsv"))) {
            for (Function f : currentProgram.getFunctionManager().getFunctions(true)) {
                if (f.isThunk() || f.isExternal()) { thunks++; continue; }
                DecompileResults r = d.decompileFunction(f, 60, monitor);
                long size = f.getBody().getNumAddresses();
                if (r != null && r.decompileCompleted()) {
                    String code = r.getDecompiledFunction().getC();
                    int n = code.split("\n").length; lines += n; ok++;
                    int warn = code.contains("WARNING") ? 1 : 0;
                    idx.println(f.getEntryPoint() + "\t" + f.getName() + "\t" + size + "\t" + n + "\tok\t" + warn);
                    c.println("// " + f.getEntryPoint()); c.println(code);
                } else {
                    bad++;
                    idx.println(f.getEntryPoint() + "\t" + f.getName() + "\t" + size + "\t0\tFAIL\t" + (r == null ? "" : r.getErrorMessage().replace('\n',' ')));
                }
            }
        }
        println("SUMMARY ok=" + ok + " fail=" + bad + " thunks=" + thunks + " lines=" + lines);
    }
}
