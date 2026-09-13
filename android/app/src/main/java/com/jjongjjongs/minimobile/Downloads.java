package com.jjongjjongs.minimobile;

import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Context;
import android.net.Uri;
import android.os.Build;
import android.os.Environment;
import android.provider.MediaStore;

import java.io.File;
import java.io.FileOutputStream;
import java.io.OutputStream;

/**
 * Puts a file in the Downloads folder, which is the one place the app can
 * write that the person using it can also reach.
 *
 * <p>From Android 10 that is a MediaStore insert and needs no permission.
 * Before it, Downloads is a plain directory and writing to it does - see
 * {@code MainActivity.withDownloadPermission}.
 */
final class Downloads {
    private Downloads() {
    }

    /** Whether {@link #write} will need the storage permission first. */
    static boolean needsPermission() {
        return Build.VERSION.SDK_INT < Build.VERSION_CODES.Q;
    }

    static void write(Context context, String name, String mimeType, byte[] contents) throws Exception {
        if (needsPermission()) {
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
        values.put(MediaStore.Downloads.MIME_TYPE, mimeType);

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
    static String safeName(String title) {
        String trimmed = title.replaceAll("[^A-Za-z0-9가-힣._ -]", "_").trim();

        if (trimmed.isEmpty()) {
            return "game";
        }

        return trimmed.length() > 60 ? trimmed.substring(0, 60).trim() : trimmed;
    }
}
