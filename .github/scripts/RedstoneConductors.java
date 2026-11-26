import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Asks a Minecraft Java server whether each of its block states conducts redstone power.
 *
 * <p>Run by {@code extract_registry.py} as a single-file source program, with the server's
 * classes on the class path. The game is reached by reflection, since an obfuscated release names
 * its classes and members as its mappings say, so the script passes each name in. It boots the
 * game's registries and asks every block state, in the order of their global ids,
 * {@code isRedstoneConductor} of an empty world, and writes one {@code 1} or {@code 0} per state
 * to the first argument's file.
 *
 * <p>Arguments: the output file, then the names of {@code SharedConstants} and its
 * {@code tryDetectVersion}, {@code Bootstrap} and its {@code bootStrap}, {@code Block} and its
 * {@code BLOCK_STATE_REGISTRY}, {@code BlockStateBase} and its {@code isRedstoneConductor},
 * {@code BlockGetter}, {@code EmptyBlockGetter} and its {@code INSTANCE}, and {@code BlockPos}
 * and its {@code ZERO}.
 */
public final class RedstoneConductors {
    private RedstoneConductors() {}

    public static void main(String[] args) throws Exception {
        if (args.length != 14) {
            throw new IllegalArgumentException(
                    "expected an output file and 13 names, got " + args.length + " arguments");
        }
        Path out = Path.of(args[0]);
        Class.forName(args[1]).getMethod(args[2]).invoke(null);
        Class.forName(args[3]).getMethod(args[4]).invoke(null);
        Iterable<?> states = (Iterable<?>) Class.forName(args[5]).getField(args[6]).get(null);
        Class<?> getter = Class.forName(args[9]);
        Class<?> pos = Class.forName(args[12]);
        Method conducts = Class.forName(args[7]).getMethod(args[8], getter, pos);
        Object level = Class.forName(args[10]).getField(args[11]).get(null);
        Object origin = pos.getField(args[13]).get(null);
        StringBuilder answers = new StringBuilder();
        for (Object state : states) {
            answers.append((Boolean) conducts.invoke(state, level, origin) ? '1' : '0');
        }
        Files.writeString(out, answers.append('\n'));
    }
}
