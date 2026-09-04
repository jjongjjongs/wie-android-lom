package com.jjongjjongs.minimobile;

import android.Manifest;
import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageManager;
import android.content.res.Configuration;
import android.database.Cursor;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Outline;
import android.graphics.Paint;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.util.Log;
import android.util.SparseArray;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewOutlineProvider;
import android.widget.Button;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;
import android.view.WindowManager;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.util.Arrays;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;
import java.nio.ShortBuffer;

/**
 * Library of imported games plus the player that runs one.
 *
 * <p>The emulator runs on a single background thread which owns
 * {@link NativeBridge#nativeStart}, {@link NativeBridge#nativeTick} and frame
 * collection. Touches post key events from the UI thread; the native side
 * queues them.
 */
public final class MainActivity extends Activity {
    private static final String TAG = "WIE-Input";

    private static final int PICK_GAME = 1001;
    private static final int REQUEST_WRITE_DOWNLOADS = 1002;
    private static final int PICK_SAVE = 1003;
    private static final int REQUEST_CALL_PHONE = 1004;

    /**
     * How long a single tick may run, and the delay scheduled after it finishes.
     * A CPU-bound title runs the whole budget every tick, so the run/idle ratio
     * is {@code BUDGET / (BUDGET + INTERVAL)}: the old 20/16 starved the emulator
     * to 56% of real time and, because MIPS is measured over the wall clock, made
     * a title that the JIT can drive at ~70 MIPS look like ~40 and feel slow. A
     * short interval lifts the duty cycle to ~83% without pegging a menu: the
     * native tick now returns the instant the emulator reports idle (every task
     * asleep), so the interval becomes a real sleep whenever there is no work.
     */
    private static final int TICK_BUDGET_MS = 20;
    private static final int TICK_INTERVAL_MS = 4;

    /** Audio commands drained per tick, so a backlog cannot stall the loop. */
    private static final int MAX_AUDIO_PER_TICK = 32;

    /** Ticks without a frame before the status line shows what tick reported. */
    private static final int STATUS_TICKS = 60;

    /** How the player splits its height between the screen and the keypad. */
    private static final float GAME_WEIGHT = 2.3f;
    private static final float KEYPAD_WEIGHT = 1f;

    /**
     * Share of the keypad's height taken by the function row, which sits where
     * the keys under a handset's screen did. Two keys over each half, so the
     * pad and the numbers below it each keep their whole half.
     */
    private static final float KEYPAD_TOP_ROW = 0.19f;

    /** How a key is painted. */
    private static final int KEY_PLAIN = 0;
    private static final int KEY_SAVE = 1;
    private static final int KEY_CLEAR = 2;
    private static final int KEY_DIRECTION = 3;
    private static final int KEY_SOFT = 4;

    // Device-dark palette shared by the library and the player, so the whole
    // app reads as one piece of hardware rather than a list over a player.
    private static final int COLOR_BG = Color.rgb(18, 19, 23);        // ground
    private static final int COLOR_PANEL = Color.rgb(28, 30, 36);     // bars, cards, rows
    private static final int COLOR_PANEL_2 = Color.rgb(35, 38, 46);   // raised surface
    private static final int COLOR_HAIR = Color.rgb(43, 46, 55);      // hairline borders
    private static final int COLOR_TEXT = Color.rgb(233, 234, 237);
    private static final int COLOR_SUBTEXT = Color.rgb(154, 156, 166);
    private static final int COLOR_ACCENT = Color.rgb(84, 199, 214);  // toggle / active
    /** Behind the emulated LCD, so its letterbox reads as a screen bezel. */
    private static final int COLOR_SCREEN_BEZEL = Color.rgb(10, 11, 14);
    /** The pad the keys sit on: dark, matching the device body. */
    private static final int COLOR_KEYPAD_TRAY = Color.rgb(23, 26, 32);

    // Light "Mini Mobile" palette for the library/home screen: a clean white
    // ground with a single green accent, matching the approved home redesign.
    // The player screen keeps the dark device palette above.
    private static final int LIB_BG = Color.rgb(255, 255, 255);
    private static final int LIB_SURFACE = Color.rgb(255, 255, 255);
    private static final int LIB_INK = Color.rgb(26, 42, 32);          // #1a2a20 primary text
    private static final int LIB_MUTED = Color.rgb(100, 117, 104);     // #647568 secondary text
    private static final int LIB_LINE = Color.rgb(233, 241, 235);      // #e9f1eb card border
    private static final int LIB_DIVIDER = Color.rgb(238, 243, 239);   // #eef3ef row divider
    private static final int LIB_GREEN = Color.rgb(46, 139, 87);       // #2e8b57 accent
    private static final int LIB_GREEN_DEEP = Color.rgb(34, 114, 71);  // #227247 accent text
    private static final int LIB_GREEN_SOFT = Color.rgb(220, 242, 226);// #dcf2e2 chip/button fill
    private static final int LIB_GREEN_LINE = Color.rgb(199, 232, 209);// #c7e8d1 button border
    private static final int LIB_GREEN_SOFTER = Color.rgb(238, 248, 241);// #eef8f1 empty tile

    private final ScheduledExecutorService emulatorThread = Executors.newSingleThreadScheduledExecutor();

    private AndroidAudioOutput audioOutput;
    private File gamesDir;
    private GameView gameView;
    private KeypadView keypad;
    private TextView playerStatus;
    private String currentGameName;
    /** The game the player is showing, kept so a rotation can relay it out. */
    private File currentGame;
    /** Which way the player is turned; the toggle in the title bar flips it. */
    private boolean landscapeMode;
    /** What is waiting on the storage permission, if anything. */
    private Runnable pendingDownload;
    /** Telephone number waiting for Android's runtime CALL_PHONE permission. */
    private String pendingPhoneCall;

    private volatile boolean running;
    private volatile boolean foreground = true;
    /** Set while the exit-confirmation dialog is up, to freeze the game. */
    private volatile boolean paused;
    private boolean playerVisible;
    private int statusCounter;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        audioOutput = new AndroidAudioOutput(this);

        gamesDir = new File(getFilesDir(), "games");
        if (!gamesDir.exists()) {
            gamesDir.mkdirs();
        }

        showLibrary();

        emulatorThread.scheduleWithFixedDelay(this::emulatorStep, 0, TICK_INTERVAL_MS, TimeUnit.MILLISECONDS);
    }

    @Override
    protected void onResume() {
        super.onResume();
        foreground = true;
        // The AudioTracks are paused, not released, when we leave the
        // foreground, so playback picks up where it left off on return.
        audioOutput.resume();
    }

    @Override
    protected void onPause() {
        foreground = false;
        // Leaving the foreground while a key is held would otherwise leave it
        // stuck down: the finger's ACTION_UP is delivered to whatever takes
        // over the screen, never to us. Release everything now so the game
        // does not read a key as held across the interruption.
        releaseKeypad();
        // Silence the tracks while backgrounded rather than letting the game's
        // BGM and effects bleed on over whatever the player switched to.
        audioOutput.pause();
        super.onPause();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        // A notification shade or dialog can take focus without pausing us,
        // and swallows the touch release the same way. Drop held keys as soon
        // as focus is lost.
        if (!hasFocus) {
            releaseKeypad();
        }
    }

    /** Releases every key the keypad currently holds, if a keypad is shown. */
    private void releaseKeypad() {
        KeypadView view = keypad;
        if (view != null) {
            view.releaseAll();
        }
    }

    @Override
    protected void onDestroy() {
        running = false;
        NativeBridge.nativeStop();
        audioOutput.release();
        emulatorThread.shutdownNow();
        super.onDestroy();
    }

    @Override
    public void onBackPressed() {
        if (!playerVisible) {
            super.onBackPressed();
            return;
        }

        // In a game, back asks before leaving. The game is frozen while the
        // dialog is up: 예 quits to the library, 아니요 (or dismissing the dialog
        // with back / an outside tap) resumes it in place.
        paused = true;
        new AlertDialog.Builder(this)
                .setTitle("종료")
                .setMessage("애플리케이션을 종료하시겠습니까?")
                .setPositiveButton("예", (dialog, which) -> exitGameToLibrary())
                .setNegativeButton("아니요", (dialog, which) -> paused = false)
                .setOnCancelListener(dialog -> paused = false)
                .show();
    }

    /** Stops the running game and returns to the library. */
    private void exitGameToLibrary() {
        running = false;
        paused = false;
        NativeBridge.nativeStop();
        audioOutput.release();
        showLibrary();
    }

    // --- library ---------------------------------------------------------

    private void showLibrary() {
        running = false;
        playerVisible = false;
        keypad = null;
        landscapeMode = false;
        // The library is always upright, whichever way the player was left.
        setRequestedOrientation(ActivityInfo.SCREEN_ORIENTATION_PORTRAIT);
        // The home screen is light, so the status-bar icons must go dark.
        setLightStatusBar(true);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(LIB_BG);

        // A small title strip at the very top, like the mockup's app bar.
        TextView bar = new TextView(this);
        bar.setText("Mini Mobile");
        bar.setTextSize(16f);
        bar.setTypeface(Typeface.DEFAULT_BOLD);
        bar.setTextColor(LIB_INK);
        bar.setPadding(dp(18), dp(12), dp(18), dp(6));
        root.addView(bar);

        LinearLayout content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setPadding(dp(16), dp(4), dp(16), dp(20));

        // Header name with the green status dot.
        LinearLayout nameRow = new LinearLayout(this);
        nameRow.setOrientation(LinearLayout.HORIZONTAL);
        nameRow.setGravity(android.view.Gravity.CENTER_VERTICAL);
        View dot = new View(this);
        dot.setBackground(circle(LIB_GREEN));
        LinearLayout.LayoutParams dotParams = new LinearLayout.LayoutParams(dp(8), dp(8));
        dotParams.rightMargin = dp(8);
        nameRow.addView(dot, dotParams);
        TextView name = new TextView(this);
        name.setText("Mini Mobile");
        name.setTextSize(19f);
        name.setTypeface(Typeface.DEFAULT_BOLD);
        name.setTextColor(LIB_INK);
        nameRow.addView(name);
        content.addView(nameRow);

        // Every header line the mockup keeps, just at smaller sizes.
        TextView sub = new TextView(this);
        sub.setText("독립 실행형 WIPI 에뮬레이터");
        sub.setTextSize(11.5f);
        sub.setTextColor(LIB_MUTED);
        sub.setPadding(0, dp(4), 0, 0);
        content.addView(sub);

        TextView store = new TextView(this);
        store.setText("게임 저장소: " + gamesDir.getAbsolutePath());
        store.setTextSize(10.5f);
        store.setTextColor(LIB_MUTED);
        store.setPadding(0, dp(1), 0, 0);
        content.addView(store);

        TextView use = new TextView(this);
        use.setText("게임 실행: 한 번 누르기 · 세이브 꺼내기·불러오기 · 삭제: 길게 누르기");
        use.setTextSize(11f);
        use.setTextColor(LIB_MUTED);
        use.setPadding(0, dp(8), 0, dp(14));
        content.addView(use);

        // Two soft-green pill buttons, side by side.
        LinearLayout actions = new LinearLayout(this);
        Button refresh = flatButton("목록 새로고침");
        refresh.setOnClickListener(v -> showLibrary());
        actions.addView(refresh, buttonParams(0));
        Button pick = flatButton("APK/ZIP 가져오기");
        pick.setOnClickListener(v -> openPicker());
        actions.addView(pick, buttonParams(dp(10)));
        content.addView(actions, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(42)));

        // Section header: title on the left, live count on the right.
        File[] games = gamesDir.listFiles(File::isFile);
        int count = games == null ? 0 : games.length;
        LinearLayout sect = new LinearLayout(this);
        sect.setOrientation(LinearLayout.HORIZONTAL);
        sect.setGravity(android.view.Gravity.CENTER_VERTICAL);
        sect.setPadding(dp(2), dp(16), dp(2), dp(8));
        TextView sectTitle = new TextView(this);
        sectTitle.setText("게임 목록");
        sectTitle.setTextSize(13f);
        sectTitle.setTypeface(Typeface.DEFAULT_BOLD);
        sectTitle.setTextColor(LIB_INK);
        sect.addView(sectTitle, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f));
        TextView sectCount = new TextView(this);
        sectCount.setText(count + "개");
        sectCount.setTextSize(12f);
        sectCount.setTextColor(LIB_MUTED);
        sect.addView(sectCount);
        content.addView(sect);

        // One rounded surface with hairline dividers between rows.
        content.addView(buildGameList(games));

        ScrollView scroll = new ScrollView(this);
        scroll.setVerticalScrollBarEnabled(false);
        scroll.addView(content);
        root.addView(scroll, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));

        applyStatusBarInset(root);
        setContentView(root);
    }

    private LinearLayout.LayoutParams buttonParams(int leftMargin) {
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f);
        params.leftMargin = leftMargin;
        return params;
    }

    /** The rounded list card, or the empty state when nothing is imported. */
    private View buildGameList(File[] games) {
        if (games == null || games.length == 0) {
            return buildEmptyState();
        }

        Arrays.sort(games, Comparator.comparing(File::getName, String.CASE_INSENSITIVE_ORDER));

        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setBackground(roundedRect(LIB_SURFACE, LIB_LINE, 1, 16));
        roundCorners(card, 16);

        for (int i = 0; i < games.length; i++) {
            card.addView(createGameRow(games[i]));
            if (i < games.length - 1) {
                View divider = new View(this);
                divider.setBackgroundColor(LIB_DIVIDER);
                card.addView(divider, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, Math.max(1, dp(1))));
            }
        }
        return card;
    }

    private View buildEmptyState() {
        LinearLayout box = new LinearLayout(this);
        box.setOrientation(LinearLayout.VERTICAL);
        box.setGravity(android.view.Gravity.CENTER_HORIZONTAL);
        box.setPadding(dp(16), dp(44), dp(16), dp(44));

        TextView tile = new TextView(this);
        tile.setText("+");
        tile.setTextSize(30f);
        tile.setTypeface(Typeface.DEFAULT_BOLD);
        tile.setTextColor(LIB_GREEN);
        tile.setGravity(android.view.Gravity.CENTER);
        tile.setBackground(roundedRect(LIB_GREEN_SOFTER, 0, 0, 20));
        box.addView(tile, new LinearLayout.LayoutParams(dp(62), dp(62)));

        TextView et = new TextView(this);
        et.setText("아직 게임이 없어요");
        et.setTextSize(15f);
        et.setTypeface(Typeface.DEFAULT_BOLD);
        et.setTextColor(LIB_INK);
        et.setGravity(android.view.Gravity.CENTER);
        et.setPadding(0, dp(12), 0, 0);
        box.addView(et);

        TextView es = new TextView(this);
        es.setText("위의 ‘APK/ZIP 가져오기’로 게임을 추가하세요.");
        es.setTextSize(12.5f);
        es.setTextColor(LIB_MUTED);
        es.setGravity(android.view.Gravity.CENTER);
        es.setPadding(0, dp(6), 0, 0);
        box.addView(es);

        return box;
    }

    private View createGameRow(File game) {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(android.view.Gravity.CENTER_VERTICAL);
        row.setPadding(dp(12), dp(10), dp(12), dp(10));

        // Cover: the archive's own icon if it carries one, otherwise a colour
        // tile with the title's first character.
        Bitmap bitmap = readArchiveIcon(game);
        View cover;
        if (bitmap != null) {
            ImageView icon = new ImageView(this);
            icon.setImageBitmap(bitmap);
            icon.setScaleType(ImageView.ScaleType.CENTER_CROP);
            roundCorners(icon, 11);
            cover = icon;
        } else {
            String label = displayName(game);
            TextView tile = new TextView(this);
            tile.setText(label.isEmpty() ? "?" : label.substring(0, 1));
            tile.setTextColor(Color.WHITE);
            tile.setTextSize(18f);
            tile.setTypeface(Typeface.DEFAULT_BOLD);
            tile.setGravity(android.view.Gravity.CENTER);
            tile.setBackground(roundedRect(colorForName(game.getName()), 0, 0, 11));
            cover = tile;
        }
        row.addView(cover, new LinearLayout.LayoutParams(dp(42), dp(42)));

        // Title, then a tag chip plus the file size.
        LinearLayout meta = new LinearLayout(this);
        meta.setOrientation(LinearLayout.VERTICAL);
        LinearLayout.LayoutParams metaParams = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f);
        metaParams.leftMargin = dp(12);
        metaParams.rightMargin = dp(10);

        TextView name = new TextView(this);
        name.setText(displayName(game));
        name.setTextColor(LIB_INK);
        name.setTextSize(14f);
        name.setTypeface(Typeface.DEFAULT_BOLD);
        name.setSingleLine(true);
        name.setEllipsize(android.text.TextUtils.TruncateAt.END);
        meta.addView(name);

        LinearLayout metaLine = new LinearLayout(this);
        metaLine.setOrientation(LinearLayout.HORIZONTAL);
        metaLine.setGravity(android.view.Gravity.CENTER_VERTICAL);
        metaLine.setPadding(0, dp(2), 0, 0);

        TextView tag = new TextView(this);
        tag.setText(archiveTag(game));
        tag.setTextSize(10f);
        tag.setTypeface(Typeface.DEFAULT_BOLD);
        tag.setTextColor(LIB_GREEN_DEEP);
        tag.setBackground(roundedRect(LIB_GREEN_SOFT, 0, 0, 6));
        tag.setPadding(dp(6), dp(1), dp(6), dp(1));
        LinearLayout.LayoutParams tagParams = new LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        tagParams.rightMargin = dp(6);
        metaLine.addView(tag, tagParams);

        TextView size = new TextView(this);
        size.setText(formatSize(game.length()));
        size.setTextSize(11.5f);
        size.setTextColor(LIB_MUTED);
        metaLine.addView(size);

        meta.addView(metaLine);
        row.addView(meta, metaParams);

        // A round, soft-green play affordance on the right.
        TextView play = new TextView(this);
        play.setText("▶");
        play.setTextSize(11f);
        play.setTextColor(LIB_GREEN);
        play.setGravity(android.view.Gravity.CENTER);
        play.setBackground(circle(LIB_GREEN_SOFT));
        row.addView(play, new LinearLayout.LayoutParams(dp(29), dp(29)));

        row.setOnClickListener(v -> showPlayer(game));
        row.setOnLongClickListener(v -> {
            showGameMenu(game);
            return true;
        });

        return row;
    }

    /** Uppercase archive extension used as the row's small tag chip. */
    private String archiveTag(File game) {
        String fileName = game.getName();
        int dot = fileName.lastIndexOf('.');
        String ext = dot >= 0 ? fileName.substring(dot + 1) : "";
        return ext.isEmpty() ? "게임" : ext.toUpperCase(java.util.Locale.ROOT);
    }

    private String formatSize(long bytes) {
        if (bytes >= 1024L * 1024L) {
            return String.format(java.util.Locale.ROOT, "%.1f MB", bytes / (1024.0 * 1024.0));
        }
        return String.format(java.util.Locale.ROOT, "%.0f KB", Math.max(1.0, bytes / 1024.0));
    }

    /** A filled, optionally bordered, rounded-rectangle background. */
    private GradientDrawable roundedRect(int fill, int stroke, int strokeDp, int radiusDp) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(fill);
        drawable.setCornerRadius(dp(radiusDp));
        if (strokeDp > 0) {
            drawable.setStroke(dp(strokeDp), stroke);
        }
        return drawable;
    }

    private GradientDrawable circle(int fill) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setShape(GradientDrawable.OVAL);
        drawable.setColor(fill);
        return drawable;
    }

    /** Clips a view to rounded corners so bitmaps and rows follow the card. */
    private void roundCorners(View view, int radiusDp) {
        final float radius = dp(radiusDp);
        view.setClipToOutline(true);
        view.setOutlineProvider(new ViewOutlineProvider() {
            @Override
            public void getOutline(View v, Outline outline) {
                outline.setRoundRect(0, 0, v.getWidth(), v.getHeight(), radius);
            }
        });
    }

    /** Dark status-bar icons for the light home screen; light for the player. */
    private void setLightStatusBar(boolean light) {
        View decor = getWindow().getDecorView();
        int flags = decor.getSystemUiVisibility();
        if (light) {
            flags |= View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR;
        } else {
            flags &= ~View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR;
        }
        decor.setSystemUiVisibility(flags);
        getWindow().setStatusBarColor(light ? LIB_BG : COLOR_PANEL);
    }

    /** What a long press offers: get the saves out, or drop the game. */
    private void showGameMenu(File game) {
        new AlertDialog.Builder(this)
                .setTitle(displayName(game))
                .setItems(new CharSequence[]{"세이브 파일 꺼내기", "세이브 불러오기", "목록에서 삭제"}, (dialog, which) -> {
                    if (which == 0) {
                        exportSaves(game);
                    } else if (which == 1) {
                        importSaves(game);
                    } else {
                        confirmDelete(game);
                    }
                })
                .setNegativeButton("취소", null)
                .show();
    }

    private void confirmDelete(File game) {
        new AlertDialog.Builder(this)
                .setTitle(displayName(game))
                .setMessage("이 게임을 목록에서 삭제할까요?\n저장한 내용은 그대로 남습니다.")
                .setNegativeButton("취소", null)
                .setPositiveButton("삭제", (dialog, which) -> {
                    game.delete();
                    showLibrary();
                })
                .show();
    }

    // --- saves -----------------------------------------------------------

    /**
     * Copies a title's saved data into Downloads, so it can be backed up or
     * moved to another phone. Saves live in the app's private directory, where
     * nothing else can reach them.
     */
    private void exportSaves(File game) {
        String title = displayName(game);

        withDownloadPermission(() -> {
            Toast.makeText(this, "세이브 파일을 꺼내는 중...", Toast.LENGTH_SHORT).show();
            exportSavesNow(game, title);
        });
    }

    private void exportSavesNow(File game, String title) {
        emulatorThread.execute(() -> {
            try {
                SaveExporter.Result result = SaveExporter.export(this, game, title);

                runOnUiThread(() -> {
                    if (result == null) {
                        Toast.makeText(this, "저장된 내용이 없습니다.", Toast.LENGTH_LONG).show();
                        return;
                    }

                    Toast.makeText(this, "다운로드 폴더에 저장: " + result.name + " (" + result.files + "개)", Toast.LENGTH_LONG).show();
                });
            } catch (Exception e) {
                runOnUiThread(() -> Toast.makeText(this, "꺼내기 실패: " + e.getMessage(), Toast.LENGTH_LONG).show());
            }
        });
    }

    /**
     * Restores saved data from a previously exported save zip, overwriting the
     * app's private save folder. The zip's own {@code db/<id>}/{@code fs/<id>}
     * paths route each file to the game it belongs to, so this works from any
     * title's menu; the file can sit in Downloads or any other folder.
     */
    private void importSaves(File game) {
        new AlertDialog.Builder(this)
                .setTitle(displayName(game))
                .setMessage("세이브 파일(.zip)을 골라 지금 저장된 내용에 덮어씁니다.\n덮어쓴 뒤에는 되돌릴 수 없습니다.")
                .setNegativeButton("취소", null)
                .setPositiveButton("파일 선택", (dialog, which) -> openSavePicker())
                .show();
    }

    private void openSavePicker() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*");
        intent.putExtra(Intent.EXTRA_MIME_TYPES, new String[]{
                "application/zip",
                "application/octet-stream",
        });
        startActivityForResult(intent, PICK_SAVE);
    }

    private void importSaveNow(Uri uri) {
        emulatorThread.execute(() -> {
            try (InputStream input = getContentResolver().openInputStream(uri)) {
                if (input == null) {
                    throw new IllegalStateException("파일을 열 수 없습니다.");
                }

                SaveImporter.Result result = SaveImporter.importZip(this, input);

                runOnUiThread(() -> Toast.makeText(
                        this,
                        "세이브를 불러왔습니다 (" + result.files + "개). 게임을 다시 시작하면 적용됩니다.",
                        Toast.LENGTH_LONG).show());
            } catch (Exception e) {
                runOnUiThread(() -> Toast.makeText(this, "불러오기 실패: " + e.getMessage(), Toast.LENGTH_LONG).show());
            }
        });
    }

    /**
     * Saves the running game's log to Downloads.
     *
     * <p>It covers this run only - the native side starts the log over when a
     * game starts - and can be taken while the game is still going, which is
     * what a title that hangs rather than stops needs.
     */
    private void saveLog() {
        withDownloadPermission(() -> {
            Toast.makeText(this, "로그를 저장하는 중...", Toast.LENGTH_SHORT).show();
            emulatorThread.execute(() -> writeLogToDownloads(null, true));
        });
    }

    /**
     * Lets the person change the {@code tracing} log filter while the app runs,
     * so capturing a module's debug/trace detail no longer needs a new APK.
     * Reached by long-pressing the log button.
     */
    private void showLogFilterDialog() {
        EditText input = new EditText(this);
        input.setText(NativeBridge.nativeLogFilter());
        input.setSingleLine(true);
        input.setSelection(input.getText().length());

        int pad = dp(16);
        FrameLayout wrap = new FrameLayout(this);
        wrap.setPadding(pad, pad / 2, pad, 0);
        wrap.addView(input);

        new AlertDialog.Builder(this)
                .setTitle("로그 필터")
                .setMessage("RUST_LOG 형식으로 무엇을 로그에 담을지 정합니다.\n예: wie_lgt=trace,wie_wipi_c=debug")
                .setView(wrap)
                .setPositiveButton("적용", (dialog, which) -> {
                    String directive = input.getText().toString().trim();
                    String error = NativeBridge.nativeSetLogFilter(directive);
                    Toast.makeText(this,
                            error.isEmpty() ? "로그 필터 적용됨" : "필터 오류: " + error,
                            Toast.LENGTH_LONG).show();
                })
                .setNeutralButton("기본값", (dialog, which) -> {
                    // An empty directive tells the native side to restore its
                    // built-in default, so the value lives in one place.
                    String error = NativeBridge.nativeSetLogFilter("");
                    Toast.makeText(this,
                            error.isEmpty() ? "기본 로그 필터로 되돌림" : "필터 오류: " + error,
                            Toast.LENGTH_LONG).show();
                })
                .setNegativeButton("취소", null)
                .show();
    }

    /**
     * Writes the current run's log to Downloads. Runs on the emulator thread so
     * the native reads happen off the UI thread.
     *
     * @param suffix   appended to the file name (before ".txt") to tell an
     *                 auto-saved crash log apart from a manual save, or null
     * @param announce whether to toast the result; an auto-save that the person
     *                 did not ask for stays quiet on failure
     */
    private void writeLogToDownloads(String suffix, boolean announce) {
        String title = currentGameName != null ? currentGameName : "wie";
        try {
            // The message shown in the title bar is cut off at one line, so the
            // whole of it goes at the top of the file.
            String error = NativeBridge.nativeLastError();
            String header = "wie " + NativeBridge.nativeVersion() + "\n"
                    + "game: " + title + "\n"
                    + "running: " + (NativeBridge.nativeRunning() != 0) + "\n"
                    + "filter: " + NativeBridge.nativeLogFilter() + "\n"
                    + (error.isEmpty() ? "" : "last error: " + error + "\n")
                    + "\n";

            byte[] contents = (header + NativeBridge.nativeLog()).getBytes("UTF-8");
            String name = Downloads.safeName(title) + " 로그" + (suffix == null ? "" : suffix) + ".txt";
            Downloads.write(this, name, "text/plain", contents);

            if (announce) {
                runOnUiThread(() -> Toast.makeText(this,
                        "다운로드 폴더에 저장: " + name + " (" + (contents.length / 1024) + "KB)",
                        Toast.LENGTH_LONG).show());
            }
        } catch (Exception e) {
            if (announce) {
                runOnUiThread(() -> Toast.makeText(this, "로그 저장 실패: " + e.getMessage(), Toast.LENGTH_LONG).show());
            }
        }
    }

    /**
     * Saves the log by itself when a game stops, so a title that crashes or ends
     * before the person can reach the log button still leaves one behind. Best
     * effort: on pre-Android 10 without the storage permission it is skipped
     * silently rather than prompting mid-crash.
     */
    private void autoSaveLogOnStop() {
        if (Downloads.needsPermission()
                && checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED) {
            return;
        }

        // A timestamp so successive crashes do not clobber one another and the
        // auto-saved file is easy to tell from a manual save.
        String stamp = new java.text.SimpleDateFormat("MMdd_HHmmss", java.util.Locale.US).format(new java.util.Date());
        writeLogToDownloads(" 자동 " + stamp, true);
    }

    /**
     * Runs {@code action} once Downloads can be written to. Before Android 10
     * that needs asking; from 10 on the MediaStore insert needs nothing.
     */
    private void withDownloadPermission(Runnable action) {
        if (!Downloads.needsPermission()
                || checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED) {
            action.run();
            return;
        }

        pendingDownload = action;
        requestPermissions(new String[]{Manifest.permission.WRITE_EXTERNAL_STORAGE}, REQUEST_WRITE_DOWNLOADS);
    }

    @Override
    public void onRequestPermissionsResult(int requestCode, String[] permissions, int[] granted) {
        super.onRequestPermissionsResult(requestCode, permissions, granted);

        if (requestCode == REQUEST_CALL_PHONE) {
            String number = pendingPhoneCall;
            pendingPhoneCall = null;

            if (number != null
                    && granted.length > 0
                    && granted[0] == PackageManager.PERMISSION_GRANTED) {
                placePhoneCall(number);
            } else if (number != null) {
                Toast.makeText(this, "전화 권한이 없어 통화 요청을 실행할 수 없습니다.", Toast.LENGTH_LONG).show();
            }
            return;
        }

        Runnable action = pendingDownload;
        pendingDownload = null;

        if (requestCode != REQUEST_WRITE_DOWNLOADS || action == null) {
            return;
        }

        if (granted.length > 0 && granted[0] == PackageManager.PERMISSION_GRANTED) {
            action.run();
        } else {
            Toast.makeText(this, "저장 공간 권한이 없어 다운로드 폴더에 쓸 수 없습니다.", Toast.LENGTH_LONG).show();
        }
    }

    /**
     * Uses the first reasonably sized PNG in the archive as cover art. Handset
     * archives ship {@code big.png}/{@code middle.png}/{@code small.png} next
     * to the descriptor.
     */
    private Bitmap readArchiveIcon(File game) {
        final int maxEntries = 200;
        final int maxIconBytes = 512 * 1024;
        final int minSide = 12;
        final int maxSide = 256;

        try (ZipInputStream zip = new ZipInputStream(new FileInputStream(game))) {
            int scanned = 0;
            ZipEntry entry;
            while ((entry = zip.getNextEntry()) != null && scanned < maxEntries) {
                scanned++;

                if (entry.isDirectory() || !entry.getName().toLowerCase().endsWith(".png")) {
                    continue;
                }
                if (entry.getSize() > maxIconBytes) {
                    continue;
                }

                ByteArrayOutputStream buffer = new ByteArrayOutputStream();
                byte[] chunk = new byte[8192];
                int read;
                while ((read = zip.read(chunk)) > 0 && buffer.size() <= maxIconBytes) {
                    buffer.write(chunk, 0, read);
                }

                byte[] bytes = buffer.toByteArray();
                Bitmap bitmap = BitmapFactory.decodeByteArray(bytes, 0, bytes.length);
                if (bitmap != null
                        && bitmap.getWidth() >= minSide && bitmap.getHeight() >= minSide
                        && bitmap.getWidth() <= maxSide && bitmap.getHeight() <= maxSide) {
                    return bitmap;
                }
            }
        } catch (Exception e) {
            // A corrupt or unreadable archive still gets a placeholder tile.
        }

        return null;
    }

    // --- import ----------------------------------------------------------

    private void openPicker() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*");
        intent.putExtra(Intent.EXTRA_MIME_TYPES, new String[]{
                "application/vnd.android.package-archive",
                "application/zip",
                "application/java-archive",
                "application/octet-stream",
        });
        startActivityForResult(intent, PICK_GAME);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);

        if (resultCode != RESULT_OK || data == null) {
            return;
        }
        if (requestCode != PICK_GAME && requestCode != PICK_SAVE) {
            return;
        }

        Uri uri = data.getData();
        if (uri == null) {
            return;
        }

        if (requestCode == PICK_SAVE) {
            Toast.makeText(this, "세이브를 불러오는 중...", Toast.LENGTH_SHORT).show();
            importSaveNow(uri);
            return;
        }

        Toast.makeText(this, "게임을 가져오는 중...", Toast.LENGTH_SHORT).show();
        emulatorThread.execute(() -> importGame(uri));
    }

    private void importGame(Uri uri) {
        String name = queryName(uri).replaceAll("[^A-Za-z0-9가-힣._ -]", "_");
        if (name.isEmpty()) {
            name = "game_" + System.currentTimeMillis() + ".zip";
        }

        File target = uniqueFile(name);
        try (InputStream input = getContentResolver().openInputStream(uri);
             FileOutputStream output = new FileOutputStream(target)) {
            if (input == null) {
                throw new IllegalStateException("파일을 열 수 없습니다.");
            }

            byte[] chunk = new byte[32768];
            int read;
            while ((read = input.read(chunk)) >= 0) {
                output.write(chunk, 0, read);
            }

            runOnUiThread(() -> {
                Toast.makeText(this, "가져오기 완료", Toast.LENGTH_SHORT).show();
                showLibrary();
            });
        } catch (Exception e) {
            target.delete();
            runOnUiThread(() -> Toast.makeText(this, "가져오기 실패: " + e.getMessage(), Toast.LENGTH_LONG).show());
        }
    }

    private String queryName(Uri uri) {
        try (Cursor cursor = getContentResolver().query(uri, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                return cursor.getString(0);
            }
        } catch (Exception e) {
            // Providers are free to reject the query; fall through to the path.
        }

        String last = uri.getLastPathSegment();
        return last != null ? last : "game.zip";
    }

    private File uniqueFile(String name) {
        File candidate = new File(gamesDir, name);
        if (!candidate.exists()) {
            return candidate;
        }

        int dot = name.lastIndexOf('.');
        String stem = dot > 0 ? name.substring(0, dot) : name;
        String extension = dot > 0 ? name.substring(dot) : "";

        for (int index = 2; ; index++) {
            candidate = new File(gamesDir, stem + " (" + index + ")" + extension);
            if (!candidate.exists()) {
                return candidate;
            }
        }
    }

    // --- player ----------------------------------------------------------

    private void showPlayer(File game) {
        playerVisible = true;
        running = false;
        paused = false;
        currentGame = game;
        currentGameName = displayName(game);
        landscapeMode = false;
        // The player is a dark device again, so restore light status-bar icons.
        setLightStatusBar(false);

        // Persistent views, kept across rotations so the last frame and any
        // held keys survive a re-layout instead of being torn down.
        gameView = new GameView(this);
        keypad = new KeypadView(this);

        // Button-driven rotation only: lock to portrait so the sensor cannot
        // turn the player on its own, then let the title-bar toggle switch it.
        setRequestedOrientation(ActivityInfo.SCREEN_ORIENTATION_PORTRAIT);
        buildPlayerContent();

        emulatorThread.execute(() -> startGame(game));
    }

    /**
     * Lays the player out for the current orientation, reusing the persistent
     * game and keypad views. Portrait stacks the screen over the keypad;
     * landscape floats the screen in the gap between the two key columns.
     */
    private void buildPlayerContent() {
        detach(gameView);
        detach(keypad);
        gameView.landscape = landscapeMode;
        keypad.landscape = landscapeMode;
        keypad.requestLayout();

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(COLOR_BG);

        root.addView(buildTitleBar(),
                new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(landscapeMode ? 40 : 46)));

        if (landscapeMode) {
            // One keypad view across the whole area with the two key columns,
            // the screen floated over the empty gap between them, so a finger
            // on each side is still one view's business.
            FrameLayout arena = new FrameLayout(this);
            arena.setBackgroundColor(COLOR_KEYPAD_TRAY);
            arena.addView(keypad,
                    new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
            FrameLayout.LayoutParams screenParams =
                    new FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.MATCH_PARENT);
            screenParams.gravity = android.view.Gravity.CENTER;
            arena.addView(gameView, screenParams);
            root.addView(arena, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));
        } else {
            root.addView(gameView, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, GAME_WEIGHT));
            root.addView(keypad, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, KEYPAD_WEIGHT));
        }

        applyStatusBarInset(root);
        setContentView(root);
    }

    /** Status line, with the rotate and log buttons in the top-right corner. */
    private View buildTitleBar() {
        LinearLayout bar = new LinearLayout(this);
        bar.setBackgroundColor(COLOR_PANEL);
        bar.setGravity(android.view.Gravity.CENTER_VERTICAL);

        playerStatus = new TextView(this);
        playerStatus.setText(running ? currentGameName : "게임을 시작하는 중...");
        playerStatus.setTextColor(COLOR_TEXT);
        playerStatus.setTextSize(15f);
        playerStatus.setGravity(android.view.Gravity.CENTER_VERTICAL);
        playerStatus.setPadding(dp(14), 0, dp(8), 0);
        playerStatus.setSingleLine(true);
        playerStatus.setEllipsize(android.text.TextUtils.TruncateAt.END);
        bar.addView(playerStatus, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f));

        Button log = navyButton("로그");
        log.setOnClickListener(v -> saveLog());
        // Long-press to change what the log captures, without a rebuild.
        log.setOnLongClickListener(v -> {
            showLogFilterDialog();
            return true;
        });
        LinearLayout.LayoutParams logParams = new LinearLayout.LayoutParams(dp(56), dp(34));
        logParams.rightMargin = dp(8);
        bar.addView(log, logParams);

        Button rotate = navyButton(landscapeMode ? "세로" : "가로");
        rotate.setOnClickListener(v -> toggleOrientation());
        LinearLayout.LayoutParams rotateParams = new LinearLayout.LayoutParams(dp(56), dp(34));
        rotateParams.rightMargin = dp(10);
        bar.addView(rotate, rotateParams);

        return bar;
    }

    /**
     * A small pill in the same dark-navy flat style as the keypad, for the
     * title bar's log and rotate buttons.
     */
    private Button navyButton(String label) {
        Button button = new Button(this);
        button.setText(label);
        button.setTextSize(13f);
        button.setAllCaps(false);
        button.setTextColor(Color.rgb(182, 194, 216));
        button.setPadding(0, 0, 0, 0);

        GradientDrawable face = new GradientDrawable(
                GradientDrawable.Orientation.TOP_BOTTOM,
                new int[] {Color.rgb(46, 57, 84), Color.rgb(38, 48, 72)});
        face.setCornerRadius(dp(8));
        face.setStroke(Math.max(1, Math.round(dp(1) * 0.8f)), Color.rgb(24, 32, 52));
        button.setBackground(face);

        return button;
    }

    /**
     * Asks for the other orientation. The window turning is what fires
     * onConfigurationChanged, which flips {@code landscapeMode} and relays the
     * player out - so that callback stays the one place the mode changes.
     */
    private void toggleOrientation() {
        setRequestedOrientation(landscapeMode
                ? ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
                : ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE);
    }

    @Override
    public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);

        // The activity is kept alive across rotation (see the manifest's
        // configChanges), so the game thread never stops - only the views move.
        if (playerVisible && gameView != null && keypad != null) {
            landscapeMode = newConfig.orientation == Configuration.ORIENTATION_LANDSCAPE;
            buildPlayerContent();
        }
    }

    private static void detach(View view) {
        if (view != null && view.getParent() instanceof ViewGroup) {
            ((ViewGroup) view.getParent()).removeView(view);
        }
    }

    private void startGame(File game) {
        try (FileInputStream input = new FileInputStream(game);
             ByteArrayOutputStream buffer = new ByteArrayOutputStream()) {
            byte[] chunk = new byte[32768];
            int read;
            while ((read = input.read(chunk)) >= 0) {
                buffer.write(chunk, 0, read);
            }

            File runtimeDir = new File(getFilesDir(), "runtime");
            if (!runtimeDir.exists()) {
                runtimeDir.mkdirs();
            }

            // A previous game's exit (onBackPressed) released the audio pump;
            // re-arm it here so this game produces sound. Idempotent for the
            // first game, where the pump is already running.
            audioOutput.start();

            // Snapshot the actual host handset model once for this emulator
            // instance so legacy PHONEMODEL queries retain their host value.
            String phoneModel = Build.MODEL != null ? Build.MODEL : "";

            String message = NativeBridge.nativeStart(
                    buffer.toByteArray(),
                    runtimeDir.getAbsolutePath(),
                    phoneModel);
            running = NativeBridge.nativeRunning() != 0;

            runOnUiThread(() -> playerStatus.setText(running ? "게임 초기화 중..." : message));
        } catch (Exception e) {
            runOnUiThread(() -> playerStatus.setText("실행 실패: " + e.getMessage()));
        }
    }

    /**
     * One scheduled step: advance the emulator, drain audio, publish a frame.
     * Runs on the emulator thread.
     */
    private void emulatorStep() {
        if (!running || !playerVisible || !foreground || paused) {
            return;
        }

        PerformanceTuner.beforeNativeTick();
        String status = NativeBridge.nativeTick(TICK_BUDGET_MS);
        PerformanceTuner.afterNativeTick();

        for (int i = 0; i < MAX_AUDIO_PER_TICK; i++) {
            byte[] command = NativeBridge.nativePollOutput();
            if (command == null) {
                break;
            }
            audioOutput.handle(command);
        }

        int backlightMode = NativeBridge.nativePollBacklightMode();
        if (backlightMode == 2) {
            runOnUiThread(() ->
                    getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON));
        }

        String phoneCall = NativeBridge.nativePollPhoneCall();
        if (phoneCall != null) {
            runOnUiThread(() -> placePhoneCall(phoneCall));
        }

        String browserUrl = NativeBridge.nativePollBrowserUrl();
        if (browserUrl != null) {
            runOnUiThread(() -> openBrowser(browserUrl));
        }

        if (NativeBridge.nativeRunning() == 0) {
            running = false;
            String error = NativeBridge.nativeLastError();
            runOnUiThread(() -> playerStatus.setText(error.isEmpty() ? "게임 실행이 중단되었습니다." : "실행 중단: " + error));
            // Already on the emulator thread; save before the log can be lost.
            autoSaveLogOnStop();
            return;
        }

        short[] frame = NativeBridge.nativeFrame();
        if (frame != null && frame.length > 2 && gameView != null) {
            runOnUiThread(() -> {
                gameView.setFrame(frame);
                playerStatus.setText(currentGameName);
            });
            return;
        }

        // Nothing painted yet: surface what tick reported, but only
        // occasionally, so a slow boot does not spam the UI thread.
        if (++statusCounter >= STATUS_TICKS) {
            statusCounter = 0;
            runOnUiThread(() -> playerStatus.setText("게임 초기화: " + status));
        }
    }

    /**
     * Host side of WIPI-C MC_phnCallPlace. The LGT WipiPlayer implementation
     * launches ACTION_CALL with exactly a tel: URI.
     */
    private void placePhoneCall(String number) {
        if (checkSelfPermission(Manifest.permission.CALL_PHONE)
                != PackageManager.PERMISSION_GRANTED) {
            pendingPhoneCall = number;
            requestPermissions(
                    new String[]{Manifest.permission.CALL_PHONE},
                    REQUEST_CALL_PHONE);
            return;
        }

        try {
            startActivity(new Intent(Intent.ACTION_CALL, Uri.parse("tel:" + number)));
        } catch (Exception e) {
            Log.e(TAG, "Phone call failed", e);
            Toast.makeText(
                    this,
                    "통화 요청 실패: " + e.getMessage(),
                    Toast.LENGTH_LONG).show();
        }
    }

    /** Host side of LGT MC_sysExecute("WAPBROWSER", argv). */
    private void openBrowser(String url) {
        try {
            startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
        } catch (Exception e) {
            Log.e(TAG, "Browser launch failed", e);
            Toast.makeText(
                    this,
                    "브라우저 실행 실패: " + e.getMessage(),
                    Toast.LENGTH_LONG).show();
        }
    }

    // --- input -----------------------------------------------------------

    // --- helpers ---------------------------------------------------------

    private Button flatButton(String label) {
        Button button = new Button(this);
        button.setText(label);
        button.setTextSize(11.5f);
        button.setTextColor(LIB_GREEN_DEEP);
        button.setAllCaps(false);
        button.setTypeface(Typeface.DEFAULT_BOLD);
        button.setBackground(roundedRect(LIB_GREEN_SOFT, LIB_GREEN_LINE, 1, 15));
        button.setPadding(dp(10), dp(6), dp(10), dp(6));
        button.setMinHeight(0);
        button.setMinimumHeight(0);
        button.setStateListAnimator(null);
        return button;
    }

    private void applyStatusBarInset(View view) {
        view.setOnApplyWindowInsetsListener((v, insets) -> {
            // Reserve all four system-bar insets, not just top and bottom: in
            // landscape a device can put its navigation bar on the left or the
            // right edge, and without the side padding the number pad would
            // slide under it.
            v.setPadding(
                    insets.getSystemWindowInsetLeft(),
                    insets.getSystemWindowInsetTop(),
                    insets.getSystemWindowInsetRight(),
                    insets.getSystemWindowInsetBottom());
            return insets;
        });
    }

    private String displayName(File file) {
        String name = file.getName();
        int dot = name.lastIndexOf('.');
        return dot > 0 ? name.substring(0, dot) : name;
    }

    /** Stable per-title tile colour for archives without cover art. */
    private int colorForName(String name) {
        int hash = name.hashCode();
        return Color.rgb(
                Math.abs(hash % 130) + 70,
                Math.abs((hash >> 8) % 130) + 70,
                Math.abs((hash >> 16) % 130) + 70);
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    // --- views -----------------------------------------------------------

    /** Draws the emulated LCD, letterboxed into whatever space it is given. */
    private final class GameView extends View {
        // No FILTER_BITMAP_FLAG: nearest-neighbour scaling keeps the low-res LCD
        // crisp (sharp pixels) instead of the blur bilinear filtering gives.
        private final Paint paint = new Paint();
        private Bitmap bitmap;
        /** In landscape the screen is centered at its own aspect; see onMeasure. */
        boolean landscape;

        GameView(MainActivity activity) {
            super(activity);
            setBackgroundColor(COLOR_SCREEN_BEZEL);
        }

        @Override
        protected void onMeasure(int widthSpec, int heightSpec) {
            // Landscape: fill the height but take only the width the screen's own
            // aspect needs, so the controls on either side stay uncovered.
            if (landscape) {
                int h = MeasureSpec.getSize(heightSpec);
                float aspect = bitmap != null && bitmap.getHeight() > 0
                        ? (float) bitmap.getWidth() / bitmap.getHeight()
                        : 240f / 320f;
                setMeasuredDimension(Math.round(h * aspect), h);
                return;
            }
            super.onMeasure(widthSpec, heightSpec);
        }

        /** @param frame {@code {width, height, RGB565 pixels...}} */
        void setFrame(short[] frame) {
            int width = frame[0] & 0xFFFF;
            int height = frame[1] & 0xFFFF;

            if (width <= 0 || height <= 0 || frame.length != width * height + 2) {
                return;
            }

            if (bitmap == null || bitmap.getWidth() != width || bitmap.getHeight() != height) {
                bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.RGB_565);
            }

            bitmap.copyPixelsFromBuffer(ShortBuffer.wrap(frame, 2, width * height));
            invalidate();
        }

        @Override
        protected void onDraw(Canvas canvas) {
            super.onDraw(canvas);

            if (bitmap == null) {
                return;
            }

            float scale = Math.min((float) getWidth() / bitmap.getWidth(), (float) getHeight() / bitmap.getHeight());
            float left = (getWidth() - bitmap.getWidth() * scale) / 2f;
            float top = (getHeight() - bitmap.getHeight() * scale) / 2f;

            canvas.drawBitmap(bitmap, null,
                    new RectF(left, top, left + bitmap.getWidth() * scale, top + bitmap.getHeight() * scale), paint);
        }
    }

    /**
     * The whole keypad, drawn and handled as one view.
     *
     * <p>It has to be one view to work at all. A grid of {@link Button}s
     * cannot take two fingers: the first view to accept a touch owns the
     * gesture, so a second finger landing on another button is delivered to
     * the first one and that button never hears about it. Diagonal movement,
     * a direction held while a number is tapped, and SEED 2's "press 0 and #"
     * are all impossible that way.
     *
     * <p>Layout is the width split in half - directions on the left, the
     * number pad on the right - under a function row that keeps the same
     * split: the two soft keys over the pad, save and back over the numbers.
     */
    private final class KeypadView extends View {
        private final Paint fill = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint ink = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Paint edge = new Paint(Paint.ANTI_ALIAS_FLAG);

        private final List<Key> keys = new ArrayList<>();
        private final SparseArray<Key> underFinger = new SparseArray<>();

        /**
         * Landscape splits the keys into two side columns with the screen in
         * the gap between them; portrait keeps the handset stack below it. It
         * stays one view either way so a finger on each side is still tracked.
         */
        boolean landscape;

        KeypadView(MainActivity activity) {
            super(activity);
            setBackgroundColor(COLOR_KEYPAD_TRAY);

            ink.setTextAlign(Paint.Align.CENTER);
            ink.setTypeface(Typeface.DEFAULT_BOLD);

            edge.setStyle(Paint.Style.STROKE);
            edge.setStrokeWidth(Math.max(1f, dp(1) * 0.8f));

            // The function row, left to right, grouped over what each pair
            // belongs with: the soft keys sit over the pad they are used
            // alongside, save and back over the numbers.
            //
            // The soft keys are the ones a handset printed nothing on - what
            // they do is whatever the game draws in the corners of its screen
            // above them. Back is the same key a handset marked C, which games
            // use both for their menu and for stepping back out of it.
            keys.add(new Key("좌상단", 5, KEY_SOFT));
            keys.add(new Key("우상단", 6, KEY_SOFT));
            keys.add(new Key("저장", 20, KEY_SAVE));
            keys.add(new Key("뒤로가기", 7, KEY_CLEAR));

            keys.add(new Key("▲", 0, KEY_DIRECTION));
            keys.add(new Key("◀", 2, KEY_DIRECTION));
            keys.add(new Key("OK", 4, KEY_PLAIN));
            keys.add(new Key("▶", 3, KEY_DIRECTION));
            keys.add(new Key("▼", 1, KEY_DIRECTION));

            for (int digit = 1; digit <= 9; digit++) {
                keys.add(new Key(String.valueOf(digit), 8 + digit, KEY_PLAIN));
            }
            keys.add(new Key("✱", 18, KEY_PLAIN));
            keys.add(new Key("0", 8, KEY_PLAIN));
            keys.add(new Key("#", 19, KEY_PLAIN));
        }

        @Override
        protected void onSizeChanged(int width, int height, int oldWidth, int oldHeight) {
            if (landscape) {
                layoutLandscape(width, height);
                return;
            }

            float pad = dp(5);
            float gap = dp(4);

            float half = (width - 2 * pad - gap) / 2f;
            float leftX = pad;
            float rightX = pad + half + gap;
            float top = pad;
            float usable = height - 2 * pad;

            float topRow = (usable - gap) * KEYPAD_TOP_ROW;
            float below = usable - gap - topRow;
            float padTop = top + topRow + gap;

            // Two function keys over each half, so the pair lines up with the
            // pad it goes with.
            float functionWidth = (half - gap) / 2f;
            place(0, leftX, top, functionWidth, topRow);
            place(1, leftX + functionWidth + gap, top, functionWidth, topRow);
            place(2, rightX, top, functionWidth, topRow);
            place(3, rightX + functionWidth + gap, top, functionWidth, topRow);

            // A three by three grid with only the plus filled in, so each
            // direction is its own key and two of them can be held at once.
            float cellWidth = (half - 2 * gap) / 3f;
            float cellHeight = (below - 2 * gap) / 3f;

            place(4, leftX + cellWidth + gap, padTop, cellWidth, cellHeight);
            place(5, leftX, padTop + cellHeight + gap, cellWidth, cellHeight);
            place(6, leftX + cellWidth + gap, padTop + cellHeight + gap, cellWidth, cellHeight);
            place(7, leftX + 2 * (cellWidth + gap), padTop + cellHeight + gap, cellWidth, cellHeight);
            place(8, leftX + cellWidth + gap, padTop + 2 * (cellHeight + gap), cellWidth, cellHeight);

            float numberWidth = (half - 2 * gap) / 3f;
            float numberHeight = (below - 3 * gap) / 4f;

            for (int index = 0; index < 12; index++) {
                float x = rightX + (index % 3) * (numberWidth + gap);
                float y = padTop + (index / 3) * (numberHeight + gap);

                place(9 + index, x, y, numberWidth, numberHeight);
            }

            ink.setTextSize(Math.min(numberHeight * 0.42f, dp(22)));
        }

        /**
         * Landscape layout: two compact control clusters, one at each edge,
         * with the screen filling the wide gap between them. Left cluster is
         * the soft keys over the direction pad; right is save and back over
         * the number pad. The keys are capped and centered in their side band
         * rather than stretched to fill it, so the screen stays the prominent
         * thing on the display.
         */
        private void layoutLandscape(int width, int height) {
            float pad = dp(10);
            float gap = dp(5);

            // The screen keeps its portrait aspect in the middle; each side
            // band is whatever is left over, and the keys sit compactly inside.
            float centerW = height * (240f / 320f);
            float sideW = (width - centerW) / 2f - pad;
            if (sideW < dp(120)) {
                sideW = (width - 2 * pad) / 2f - dp(60);
            }
            float leftX = pad;
            float rightX = width - pad - sideW;

            float usable = height - 2 * pad;

            // A hard cap keeps the keys small; the two size limits keep them
            // inside the band's width and inside its height (the taller, right
            // cluster is one function row over four number rows = 4.8 cells).
            float keyCap = dp(54);
            float cell = Math.min(keyCap, Math.min((sideW - 2 * gap) / 3f, (usable - 4 * gap) / 4.8f));
            float funcH = cell * 0.8f;

            float clusterW = cell * 3 + gap * 2;
            float functionWidth = (clusterW - gap) / 2f;
            float leftClusterX = leftX + (sideW - clusterW) / 2f;
            float rightClusterX = rightX + (sideW - clusterW) / 2f;

            float leftHeight = funcH + 3 * cell + 3 * gap;
            float rightHeight = funcH + 4 * cell + 4 * gap;
            float leftTop = pad + (usable - leftHeight) / 2f;
            float rightTop = pad + (usable - rightHeight) / 2f;

            // LEFT cluster: soft keys over the direction pad.
            place(0, leftClusterX, leftTop, functionWidth, funcH);
            place(1, leftClusterX + functionWidth + gap, leftTop, functionWidth, funcH);

            float dx = leftClusterX;
            float dy = leftTop + funcH + gap;
            place(4, dx + cell + gap, dy, cell, cell);
            place(5, dx, dy + cell + gap, cell, cell);
            place(6, dx + cell + gap, dy + cell + gap, cell, cell);
            place(7, dx + 2 * (cell + gap), dy + cell + gap, cell, cell);
            place(8, dx + cell + gap, dy + 2 * (cell + gap), cell, cell);

            // RIGHT cluster: save and back over the number pad.
            place(2, rightClusterX, rightTop, functionWidth, funcH);
            place(3, rightClusterX + functionWidth + gap, rightTop, functionWidth, funcH);

            float nx = rightClusterX;
            float ny = rightTop + funcH + gap;
            for (int index = 0; index < 12; index++) {
                float x = nx + (index % 3) * (cell + gap);
                float y = ny + (index / 3) * (cell + gap);
                place(9 + index, x, y, cell, cell);
            }

            ink.setTextSize(Math.min(cell * 0.42f, dp(20)));
        }

        private void place(int index, float x, float y, float width, float height) {
            Key key = keys.get(index);
            key.bounds.set(x, y, x + width, y + height);
            key.shade();
        }

        @Override
        protected void onDraw(Canvas canvas) {
            float radius = dp(8);

            for (Key key : keys) {
                if (key.down) {
                    fill.setShader(null);
                    fill.setColor(key.pressedColor());
                } else {
                    fill.setShader(key.shader);
                    fill.setColor(Color.WHITE);
                }
                canvas.drawRoundRect(key.bounds, radius, radius, fill);
                fill.setShader(null);

                edge.setColor(key.borderColor());
                canvas.drawRoundRect(key.bounds, radius, radius, edge);

                ink.setColor(key.textColor());

                // A label wider than its key is shrunk to fit rather than
                // clipped, so a word can be used where a digit was.
                float was = ink.getTextSize();
                float limit = key.bounds.width() * 0.82f;
                float measured = ink.measureText(key.label);
                if (measured > limit && measured > 0f) {
                    ink.setTextSize(was * limit / measured);
                }

                canvas.drawText(key.label, key.bounds.centerX(), key.bounds.centerY() + ink.getTextSize() * 0.36f, ink);
                ink.setTextSize(was);
            }
        }

        @Override
        public boolean onTouchEvent(MotionEvent event) {
            switch (event.getActionMasked()) {
                case MotionEvent.ACTION_DOWN:
                case MotionEvent.ACTION_POINTER_DOWN: {
                    int pointer = event.getActionIndex();
                    underFinger.put(event.getPointerId(pointer), keyAt(event.getX(pointer), event.getY(pointer)));
                    break;
                }
                case MotionEvent.ACTION_MOVE: {
                    for (int pointer = 0; pointer < event.getPointerCount(); pointer++) {
                        underFinger.put(event.getPointerId(pointer), keyAt(event.getX(pointer), event.getY(pointer)));
                    }
                    break;
                }
                case MotionEvent.ACTION_UP:
                case MotionEvent.ACTION_POINTER_UP: {
                    underFinger.remove(event.getPointerId(event.getActionIndex()));
                    break;
                }
                case MotionEvent.ACTION_CANCEL: {
                    underFinger.clear();
                    break;
                }
                default:
                    return true;
            }

            settle();
            return true;
        }

        private Key keyAt(float x, float y) {
            for (Key key : keys) {
                if (key.bounds.contains(x, y)) {
                    return key;
                }
            }
            return null;
        }

        /**
         * Forgets every finger and sends the resulting key-ups, for when the
         * player is interrupted mid-press and the real ACTION_UP will never
         * arrive.
         */
        void releaseAll() {
            if (underFinger.size() == 0) {
                return;
            }
            underFinger.clear();
            settle();
        }

        /**
         * Sends the difference between what is held now and what was held
         * before, so a finger sliding off a key releases it and two fingers on
         * one key still press it once.
         */
        private void settle() {
            boolean changed = false;

            for (Key key : keys) {
                boolean held = false;
                for (int index = 0; index < underFinger.size(); index++) {
                    if (underFinger.valueAt(index) == key) {
                        held = true;
                        break;
                    }
                }

                if (held == key.down) {
                    continue;
                }

                key.down = held;
                changed = true;
                Log.d(TAG, (held ? "key down: " : "key up: ") + key.code);
                NativeBridge.nativeKey(key.code, held ? 1 : 0);
            }

            if (changed) {
                invalidate();
            }
        }
    }

    /** One key of {@link KeypadView}. */
    private static final class Key {
        final String label;
        final int code;
        final int style;
        final RectF bounds = new RectF();
        android.graphics.Shader shader;
        boolean down;

        Key(String label, int code, int style) {
            this.label = label;
            this.code = code;
            this.style = style;
        }

        /** Rebuilds the face gradient for the bounds the key was just given. */
        void shade() {
            shader = new android.graphics.LinearGradient(
                    0, bounds.top, 0, bounds.bottom,
                    topColor(), bottomColor(), android.graphics.Shader.TileMode.CLAMP);
        }

        // Every key is a flat dark-navy face with light blue-gray text; save
        // and back carry only a faint green / red cast within the same family
        // so they still read apart from the rest at a glance. The top/bottom
        // pair keeps the barest gradient so a face has some depth without
        // looking glossy.
        private int topColor() {
            switch (style) {
                case KEY_SAVE:
                    return Color.rgb(45, 74, 63);
                case KEY_CLEAR:
                    return Color.rgb(78, 51, 60);
                case KEY_SOFT:
                    return Color.rgb(52, 64, 92);
                case KEY_DIRECTION:
                    return Color.rgb(44, 56, 84);
                default:
                    return Color.rgb(46, 57, 84);
            }
        }

        private int bottomColor() {
            switch (style) {
                case KEY_SAVE:
                    return Color.rgb(37, 62, 53);
                case KEY_CLEAR:
                    return Color.rgb(66, 43, 51);
                case KEY_SOFT:
                    return Color.rgb(43, 54, 80);
                case KEY_DIRECTION:
                    return Color.rgb(36, 46, 72);
                default:
                    return Color.rgb(38, 48, 72);
            }
        }

        int borderColor() {
            switch (style) {
                case KEY_SAVE:
                    return Color.rgb(28, 52, 44);
                case KEY_CLEAR:
                    return Color.rgb(52, 32, 38);
                case KEY_SOFT:
                    return Color.rgb(30, 40, 62);
                default:
                    return Color.rgb(24, 32, 52);
            }
        }

        int pressedColor() {
            switch (style) {
                case KEY_SAVE:
                    return Color.rgb(60, 110, 90);
                case KEY_CLEAR:
                    return Color.rgb(120, 70, 82);
                case KEY_DIRECTION:
                    return Color.rgb(70, 92, 132);
                default:
                    return Color.rgb(66, 82, 120);
            }
        }

        int textColor() {
            switch (style) {
                case KEY_SAVE:
                    return Color.rgb(180, 214, 196);
                case KEY_CLEAR:
                    return Color.rgb(220, 186, 190);
                case KEY_DIRECTION:
                    return Color.rgb(198, 210, 230);
                default:
                    return Color.rgb(182, 194, 216);
            }
        }
    }
}
