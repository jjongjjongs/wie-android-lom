package com.jjongjjongs.wiemobile;

import android.Manifest;
import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.database.Cursor;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.graphics.drawable.ColorDrawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.util.Log;
import android.util.SparseArray;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;

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

    /** How long a single tick may run, and how often ticks are scheduled. */
    private static final int TICK_BUDGET_MS = 20;
    private static final int TICK_INTERVAL_MS = 16;

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

    private static final int COLOR_BG = Color.rgb(47, 47, 47);
    private static final int COLOR_PANEL = Color.rgb(35, 35, 35);
    private static final int COLOR_TEXT = Color.rgb(232, 232, 232);
    private static final int COLOR_SUBTEXT = Color.rgb(190, 190, 190);

    /** Handset keypad: light keys with a black face, dark keys for the pad. */
    private static final int COLOR_KEYPAD_TRAY = Color.rgb(186, 186, 186);

    private final ScheduledExecutorService emulatorThread = Executors.newSingleThreadScheduledExecutor();

    private AndroidAudioOutput audioOutput;
    private File gamesDir;
    private GameView gameView;
    private KeypadView keypad;
    private TextView playerStatus;
    private String currentGameName;
    /** What is waiting on the storage permission, if anything. */
    private Runnable pendingDownload;

    private volatile boolean running;
    private volatile boolean foreground = true;
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

        running = false;
        NativeBridge.nativeStop();
        audioOutput.release();
        showLibrary();
    }

    // --- library ---------------------------------------------------------

    private void showLibrary() {
        running = false;
        playerVisible = false;
        keypad = null;

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(COLOR_BG);

        TextView bar = new TextView(this);
        bar.setText("WIE WIPI Player");
        bar.setTextSize(21f);
        bar.setTextColor(Color.WHITE);
        bar.setGravity(android.view.Gravity.CENTER_VERTICAL);
        bar.setPadding(dp(18), 0, dp(18), 0);
        bar.setBackgroundColor(COLOR_PANEL);
        root.addView(bar, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(58)));

        LinearLayout header = new LinearLayout(this);
        header.setOrientation(LinearLayout.VERTICAL);
        header.setPadding(dp(18), dp(20), dp(18), dp(8));

        TextView title = new TextView(this);
        title.setText("WIE WIPI Player");
        title.setTextSize(27f);
        title.setTextColor(Color.WHITE);
        title.setGravity(android.view.Gravity.CENTER_HORIZONTAL);
        header.addView(title);

        TextView about = new TextView(this);
        about.setText("독립 실행형 WIPI 에뮬레이터\n게임 저장소: " + gamesDir.getAbsolutePath());
        about.setTextSize(14f);
        about.setTextColor(COLOR_SUBTEXT);
        about.setPadding(0, dp(18), 0, 0);
        header.addView(about);

        TextView hint = new TextView(this);
        hint.setText("게임 실행: 한 번 누르기\n세이브 꺼내기 · 삭제: 길게 누르기");
        hint.setTextSize(14f);
        hint.setTextColor(COLOR_SUBTEXT);
        hint.setPadding(0, dp(16), 0, dp(14));
        header.addView(hint);

        LinearLayout actions = new LinearLayout(this);

        Button refresh = flatButton("목록 새로고침");
        refresh.setOnClickListener(v -> showLibrary());
        actions.addView(refresh, buttonParams(0));

        Button pick = flatButton("APK/ZIP 가져오기");
        pick.setOnClickListener(v -> openPicker());
        actions.addView(pick, buttonParams(dp(12)));

        header.addView(actions, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(56)));
        root.addView(header);

        LinearLayout list = new LinearLayout(this);
        list.setOrientation(LinearLayout.VERTICAL);
        populateGames(list);

        ScrollView scroll = new ScrollView(this);
        scroll.addView(list);
        root.addView(scroll, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));

        applyStatusBarInset(root);
        setContentView(root);
    }

    private LinearLayout.LayoutParams buttonParams(int leftMargin) {
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f);
        params.leftMargin = leftMargin;
        return params;
    }

    private void populateGames(LinearLayout list) {
        File[] games = gamesDir.listFiles(File::isFile);

        if (games == null || games.length == 0) {
            TextView empty = new TextView(this);
            empty.setText("가져온 게임이 없습니다.");
            empty.setTextColor(COLOR_SUBTEXT);
            empty.setTextSize(16f);
            empty.setGravity(android.view.Gravity.CENTER);
            empty.setPadding(0, dp(48), 0, 0);
            list.addView(empty);
            return;
        }

        Arrays.sort(games, Comparator.comparing(File::getName, String.CASE_INSENSITIVE_ORDER));
        for (File game : games) {
            list.addView(createGameRow(game));
        }
    }

    private View createGameRow(File game) {
        LinearLayout row = new LinearLayout(this);
        row.setGravity(android.view.Gravity.CENTER_VERTICAL);
        row.setPadding(dp(18), dp(10), dp(18), dp(10));

        ImageView icon = new ImageView(this);
        Bitmap bitmap = readArchiveIcon(game);
        if (bitmap != null) {
            icon.setImageBitmap(bitmap);
            icon.setScaleType(ImageView.ScaleType.CENTER_CROP);
        } else {
            icon.setImageDrawable(new ColorDrawable(colorForName(game.getName())));
        }
        row.addView(icon, new LinearLayout.LayoutParams(dp(44), dp(44)));

        TextView name = new TextView(this);
        name.setText(displayName(game));
        name.setTextColor(COLOR_TEXT);
        name.setTextSize(17f);
        name.setPadding(dp(14), 0, 0, 0);
        row.addView(name, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f));

        row.setOnClickListener(v -> showPlayer(game));
        row.setOnLongClickListener(v -> {
            showGameMenu(game);
            return true;
        });

        return row;
    }

    /** What a long press offers: get the saves out, or drop the game. */
    private void showGameMenu(File game) {
        new AlertDialog.Builder(this)
                .setTitle(displayName(game))
                .setItems(new CharSequence[]{"세이브 파일 꺼내기", "목록에서 삭제"}, (dialog, which) -> {
                    if (which == 0) {
                        exportSaves(game);
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
     * Saves the running game's log to Downloads.
     *
     * <p>It covers this run only - the native side starts the log over when a
     * game starts - and can be taken while the game is still going, which is
     * what a title that hangs rather than stops needs.
     */
    private void saveLog() {
        String title = currentGameName != null ? currentGameName : "wie";

        withDownloadPermission(() -> {
            Toast.makeText(this, "로그를 저장하는 중...", Toast.LENGTH_SHORT).show();

            emulatorThread.execute(() -> {
                try {
                    // The message shown in the title bar is cut off at one
                    // line, so the whole of it goes at the top of the file.
                    String error = NativeBridge.nativeLastError();
                    String header = "wie " + NativeBridge.nativeVersion() + "\n"
                            + "game: " + title + "\n"
                            + "running: " + (NativeBridge.nativeRunning() != 0) + "\n"
                            + (error.isEmpty() ? "" : "last error: " + error + "\n")
                            + "\n";

                    byte[] contents = (header + NativeBridge.nativeLog()).getBytes("UTF-8");
                    String name = Downloads.safeName(title) + " 로그.txt";
                    Downloads.write(this, name, "text/plain", contents);

                    runOnUiThread(() -> Toast.makeText(this,
                            "다운로드 폴더에 저장: " + name + " (" + (contents.length / 1024) + "KB)",
                            Toast.LENGTH_LONG).show());
                } catch (Exception e) {
                    runOnUiThread(() -> Toast.makeText(this, "로그 저장 실패: " + e.getMessage(), Toast.LENGTH_LONG).show());
                }
            });
        });
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

        if (requestCode != PICK_GAME || resultCode != RESULT_OK || data == null) {
            return;
        }

        Uri uri = data.getData();
        if (uri == null) {
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
        currentGameName = displayName(game);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(Color.BLACK);

        // The title bar doubles as the status line: it carries the game's name
        // once there is one, and what the loader is doing until then. A message
        // long enough to matter does not fit, which is what the log is for.
        LinearLayout titleBar = new LinearLayout(this);
        titleBar.setBackgroundColor(Color.BLACK);
        titleBar.setGravity(android.view.Gravity.CENTER_VERTICAL);

        playerStatus = new TextView(this);
        playerStatus.setText("게임을 시작하는 중...");
        playerStatus.setTextColor(COLOR_TEXT);
        playerStatus.setTextSize(16f);
        playerStatus.setGravity(android.view.Gravity.CENTER_VERTICAL);
        playerStatus.setPadding(dp(14), 0, dp(8), 0);
        playerStatus.setSingleLine(true);
        playerStatus.setEllipsize(android.text.TextUtils.TruncateAt.END);
        titleBar.addView(playerStatus, new LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f));

        Button log = flatButton("로그");
        log.setTextSize(13f);
        log.setOnClickListener(v -> saveLog());
        LinearLayout.LayoutParams logParams = new LinearLayout.LayoutParams(dp(64), dp(34));
        logParams.rightMargin = dp(10);
        titleBar.addView(log, logParams);

        root.addView(titleBar, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(46)));

        gameView = new GameView(this);
        root.addView(gameView, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, GAME_WEIGHT));

        keypad = new KeypadView(this);
        root.addView(keypad, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, KEYPAD_WEIGHT));

        applyStatusBarInset(root);
        setContentView(root);

        emulatorThread.execute(() -> startGame(game));
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

            String message = NativeBridge.nativeStart(buffer.toByteArray(), runtimeDir.getAbsolutePath());
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
        if (!running || !playerVisible || !foreground) {
            return;
        }

        String status = NativeBridge.nativeTick(TICK_BUDGET_MS);

        for (int i = 0; i < MAX_AUDIO_PER_TICK; i++) {
            byte[] command = NativeBridge.nativePollOutput();
            if (command == null) {
                break;
            }
            audioOutput.handle(command);
        }

        if (NativeBridge.nativeRunning() == 0) {
            running = false;
            String error = NativeBridge.nativeLastError();
            runOnUiThread(() -> playerStatus.setText(error.isEmpty() ? "게임 실행이 중단되었습니다." : "실행 중단: " + error));
            return;
        }

        int[] frame = NativeBridge.nativeFrame();
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

    // --- input -----------------------------------------------------------

    // --- helpers ---------------------------------------------------------

    private Button flatButton(String label) {
        Button button = new Button(this);
        button.setText(label);
        button.setTextSize(16f);
        button.setTextColor(Color.rgb(30, 30, 30));
        button.setAllCaps(false);
        button.setBackgroundColor(Color.rgb(211, 211, 211));
        return button;
    }

    private void applyStatusBarInset(View view) {
        view.setOnApplyWindowInsetsListener((v, insets) -> {
            v.setPadding(0, insets.getSystemWindowInsetTop(), 0, insets.getSystemWindowInsetBottom());
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
        private final Paint paint = new Paint(Paint.FILTER_BITMAP_FLAG);
        private Bitmap bitmap;

        GameView(MainActivity activity) {
            super(activity);
            setBackgroundColor(Color.WHITE);
        }

        /** @param frame {@code {width, height, ARGB_8888 pixels...}} */
        void setFrame(int[] frame) {
            int width = frame[0];
            int height = frame[1];

            if (width <= 0 || height <= 0 || frame.length != width * height + 2) {
                return;
            }

            if (bitmap == null || bitmap.getWidth() != width || bitmap.getHeight() != height) {
                bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888);
            }

            bitmap.setPixels(frame, 2, width, 0, 0, width, height);
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

        private void place(int index, float x, float y, float width, float height) {
            Key key = keys.get(index);
            key.bounds.set(x, y, x + width, y + height);
            key.shade();
        }

        @Override
        protected void onDraw(Canvas canvas) {
            float radius = dp(5);

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

        private int topColor() {
            switch (style) {
                case KEY_SAVE:
                    return Color.rgb(63, 143, 82);
                case KEY_SOFT:
                    return Color.rgb(133, 133, 133);
                case KEY_CLEAR:
                    return Color.rgb(154, 64, 64);
                case KEY_DIRECTION:
                    return Color.rgb(108, 108, 108);
                default:
                    return Color.rgb(251, 251, 251);
            }
        }

        private int bottomColor() {
            switch (style) {
                case KEY_SAVE:
                    return Color.rgb(44, 107, 59);
                case KEY_SOFT:
                    return Color.rgb(104, 104, 104);
                case KEY_CLEAR:
                    return Color.rgb(122, 47, 47);
                case KEY_DIRECTION:
                    return Color.rgb(82, 82, 82);
                default:
                    return Color.rgb(210, 210, 210);
            }
        }

        int borderColor() {
            switch (style) {
                case KEY_SAVE:
                    return Color.rgb(36, 82, 49);
                case KEY_SOFT:
                    return Color.rgb(86, 86, 86);
                case KEY_CLEAR:
                    return Color.rgb(94, 35, 35);
                case KEY_DIRECTION:
                    return Color.rgb(68, 68, 68);
                default:
                    return Color.rgb(169, 169, 169);
            }
        }

        int pressedColor() {
            switch (style) {
                case KEY_SAVE:
                    return Color.rgb(92, 180, 112);
                case KEY_SOFT:
                    return Color.rgb(168, 168, 168);
                case KEY_CLEAR:
                    return Color.rgb(190, 90, 90);
                case KEY_DIRECTION:
                    return Color.rgb(140, 140, 140);
                default:
                    return Color.rgb(160, 160, 160);
            }
        }

        int textColor() {
            return style == KEY_PLAIN ? Color.rgb(22, 24, 28) : Color.WHITE;
        }
    }
}
