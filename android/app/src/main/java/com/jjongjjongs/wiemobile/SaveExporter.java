package com.jjongjjongs.wiemobile;

import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Context;
import android.net.Uri;
import android.os.Build;
import android.os.Environment;
import android.provider.MediaStore;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.List;
import java.util.zip.ZipEntry;
import java.util.zip.ZipOutputStream;

/**
 * Copies a title's saved data out to the Downloads folder.
 *
 * <p>Saves live in the app's private directory, where nothing else can reach
 * them: the emulator writes record stores under {@code runtime/db/<product
 * id>} and files the game itself wrote under {@code runtime/fs/<application
 * id>}. Both are collected into one zip, because a title may use either or
 * both and which one it used is not something the player can be expected to
 * know.
 *
 * <p>The two ids come from {@link NativeBridge#nativeSaveIds}: an archive names
 * itself in its own descriptor, and only the loader reads that.
 */
final class SaveExporter {
    /** Bound on one file copied into the zip, so a bad path cannot fill memory. */
    private static final int CHUNK = 32768;

    /** What the export is called, and what it holds. */
    static final class Result {
        final String name;
        final int files;

        private Result(String name, int files) {
            this.name = name;
            this.files = files;
        }
    }

    private SaveExporter() {
    }

    /**
     * Writes {@code <title> 세이브.zip} to Downloads.
     *
     * @param archive  the imported game file, read to find its ids
     * @param title    display name, used for the zip's name
     * @return what was written, or {@code null} if the title has saved nothing
     * @throws Exception if reading or writing failed; the message is shown as-is
     */
    static Result export(Context context, File archive, String title) throws Exception {
        String[] ids = readIds(archive);
        List<File> roots = new ArrayList<>();

        File runtime = new File(context.getFilesDir(), "runtime");
        addIfPresent(roots, new File(new File(runtime, "db"), ids[0]));
        // The two ids are equal for the formats that carry only one, and the
        // same directory must not go in twice.
        if (!ids[1].equals(ids[0])) {
            addIfPresent(roots, new File(new File(runtime, "fs"), ids[1]));
        }
        addIfPresent(roots, new File(new File(runtime, "fs"), ids[0]));

        if (roots.isEmpty()) {
            return null;
        }

        ByteArrayOutputStream buffer = new ByteArrayOutputStream();
        int files;
        try (ZipOutputStream zip = new ZipOutputStream(buffer)) {
            files = 0;
            for (File root : roots) {
                // The zip keeps the db/fs split, so an export can be told apart
                // from another and put back by hand if it comes to that.
                files += addTree(zip, root, root.getParentFile().getName() + "/" + root.getName());
            }
        }

        if (files == 0) {
            return null;
        }

        String name = safeName(title) + " 세이브.zip";
        write(context, name, buffer.toByteArray());

        return new Result(name, files);
    }

    private static String[] readIds(File archive) throws Exception {
        byte[] bytes;
        try (InputStream input = new FileInputStream(archive); ByteArrayOutputStream buffer = new ByteArrayOutputStream()) {
            byte[] chunk = new byte[CHUNK];
            int read;
            while ((read = input.read(chunk)) >= 0) {
                buffer.write(chunk, 0, read);
            }
            bytes = buffer.toByteArray();
        }

        String reported = NativeBridge.nativeSaveIds(bytes);
        String[] ids = reported.split("\n", -1);
        if (ids.length < 2 || ids[0].isEmpty()) {
            throw new IllegalStateException("이 파일의 저장 위치를 알 수 없습니다.");
        }

        return new String[]{ids[0], ids[1].isEmpty() ? ids[0] : ids[1]};
    }

    private static void addIfPresent(List<File> roots, File directory) {
        if (directory.isDirectory() && !roots.contains(directory)) {
            roots.add(directory);
        }
    }

    /** Adds every file under {@code root}, and returns how many there were. */
    private static int addTree(ZipOutputStream zip, File root, String prefix) throws Exception {
        File[] entries = root.listFiles();
        if (entries == null) {
            return 0;
        }

        int written = 0;
        for (File entry : entries) {
            String path = prefix + "/" + entry.getName();

            if (entry.isDirectory()) {
                written += addTree(zip, entry, path);
                continue;
            }

            zip.putNextEntry(new ZipEntry(path));
            try (InputStream input = new FileInputStream(entry)) {
                byte[] chunk = new byte[CHUNK];
                int read;
                while ((read = input.read(chunk)) >= 0) {
                    zip.write(chunk, 0, read);
                }
            }
            zip.closeEntry();
            written++;
        }

        return written;
    }

    /**
     * Puts the zip in Downloads. From Android 10 that is a MediaStore insert
     * and needs no permission; before it, Downloads is a plain directory.
     */
    private static void write(Context context, String name, byte[] contents) throws Exception {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            File downloads = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS);
            if (!downloads.exists() && !downloads.mkdirs()) {
                throw new IllegalStateException("다운로드 폴더를 열 수 없습니다.");
            }

            try (FileOutputStream output = new FileOutputStream(new File(downloads, name))) {
                output.write(contents);
            }
            return;
        }

        ContentValues values = new ContentValues();
        values.put(MediaStore.Downloads.DISPLAY_NAME, name);
        values.put(MediaStore.Downloads.MIME_TYPE, "application/zip");

        ContentResolver resolver = context.getContentResolver();
        Uri target = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values);
        if (target == null) {
            throw new IllegalStateException("다운로드 폴더에 쓸 수 없습니다.");
        }

        try (OutputStream output = resolver.openOutputStream(target)) {
            if (output == null) {
                throw new IllegalStateException("다운로드 폴더에 쓸 수 없습니다.");
            }
            output.write(contents);
        } catch (Exception e) {
            // A half-written entry would show up in Downloads as a broken file.
            resolver.delete(target, null, null);
            throw e;
        }
    }

    /** Trims a title down to something a filesystem will take. */
    private static String safeName(String title) {
        String trimmed = title.replaceAll("[^A-Za-z0-9가-힣._ -]", "_").trim();

        if (trimmed.isEmpty()) {
            return "game";
        }

        return trimmed.length() > 60 ? trimmed.substring(0, 60).trim() : trimmed;
    }
}
