package com.jjongjjongs.wiemobile;

import android.content.Context;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

/**
 * Restores a title's saved data from an exported save zip back into the app's
 * private save directory, overwriting what is there.
 *
 * <p>The inverse of {@link SaveExporter}. The export keeps the {@code db/<id>}
 * and {@code fs/<id>} split in the zip's own entry paths, and each id names the
 * game the entry belongs to, so an import routes itself by the path alone: the
 * same save always lands under {@code runtime/db|fs/<id>} no matter which game's
 * menu opened the picker. The picked file comes through the Storage Access
 * Framework, so it may sit in Downloads or any other folder the user reaches.
 */
final class SaveImporter {
    /** Bound on one file written out of the zip, so a bad entry cannot fill memory. */
    private static final int CHUNK = 32768;

    /** What the import put back. */
    static final class Result {
        final int files;

        private Result(int files) {
            this.files = files;
        }
    }

    private SaveImporter() {
    }

    /**
     * Extracts every {@code db/...} and {@code fs/...} entry of the zip read
     * from {@code input} into the app's {@code runtime} directory, overwriting
     * files already there.
     *
     * @return how many files were restored
     * @throws Exception if the stream is not a save zip, or writing failed; the
     *                   message is shown to the player as-is
     */
    static Result importZip(Context context, InputStream input) throws Exception {
        File runtime = new File(context.getFilesDir(), "runtime");
        // Trailing separator so a prefix test cannot match a sibling directory
        // whose name merely starts with "runtime".
        String runtimePath = runtime.getCanonicalPath() + File.separator;

        int files = 0;
        try (ZipInputStream zip = new ZipInputStream(input)) {
            ZipEntry entry;
            while ((entry = zip.getNextEntry()) != null) {
                String name = entry.getName();

                // Only the save trees are restored; anything else in the zip is
                // ignored so a stray archive cannot scatter files into runtime.
                if (entry.isDirectory() || !(name.startsWith("db/") || name.startsWith("fs/"))) {
                    continue;
                }

                File target = new File(runtime, name);

                // A zip is untrusted input: refuse any entry whose resolved path
                // would escape the runtime directory (a "../" traversal).
                if (!target.getCanonicalPath().startsWith(runtimePath)) {
                    throw new IllegalStateException("세이브 파일에 잘못된 경로가 들어 있습니다.");
                }

                File parent = target.getParentFile();
                if (parent != null && !parent.isDirectory() && !parent.mkdirs()) {
                    throw new IllegalStateException("폴더를 만들 수 없습니다: " + parent.getName());
                }

                try (FileOutputStream output = new FileOutputStream(target)) {
                    byte[] chunk = new byte[CHUNK];
                    int read;
                    while ((read = zip.read(chunk)) >= 0) {
                        output.write(chunk, 0, read);
                    }
                }

                files++;
            }
        }

        if (files == 0) {
            throw new IllegalStateException("세이브 데이터가 없는 파일입니다.");
        }

        return new Result(files);
    }
}
