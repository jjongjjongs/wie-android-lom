package com.jjongjjongs.wiemobile;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
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
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.util.Log;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.GridLayout;
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
import java.util.Comparator;
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

    /** How long a single tick may run, and how often ticks are scheduled. */
    private static final int TICK_BUDGET_MS = 20;
    private static final int TICK_INTERVAL_MS = 16;

    /** Audio commands drained per tick, so a backlog cannot stall the loop. */
    private static final int MAX_AUDIO_PER_TICK = 32;

    /** Ticks without a frame before the status line shows what tick reported. */
    private static final int STATUS_TICKS = 60;

    private static final int COLOR_BG = Color.rgb(47, 47, 47);
    private static final int COLOR_PANEL = Color.rgb(35, 35, 35);
    private static final int COLOR_TEXT = Color.rgb(232, 232, 232);
    private static final int COLOR_SUBTEXT = Color.rgb(190, 190, 190);

    private final ScheduledExecutorService emulatorThread = Executors.newSingleThreadScheduledExecutor();

    private AndroidAudioOutput audioOutput;
    private File gamesDir;
    private GameView gameView;
    private TextView playerStatus;
    private String currentGameName;

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
    }

    @Override
    protected void onPause() {
        foreground = false;
        super.onPause();
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

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(COLOR_BG);

        TextView title = new TextView(this);
        title.setText("WIE WIPI Player");
        title.setTextSize(21f);
        title.setTextColor(Color.WHITE);
        title.setGravity(android.view.Gravity.CENTER_VERTICAL);
        title.setPadding(dp(18), 0, dp(18), 0);
        title.setBackgroundColor(COLOR_PANEL);
        root.addView(title, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(58)));

        LinearLayout list = new LinearLayout(this);
        list.setOrientation(LinearLayout.VERTICAL);
        populateGames(list);

        ScrollView scroll = new ScrollView(this);
        scroll.addView(list);
        root.addView(scroll, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));

        LinearLayout actions = new LinearLayout(this);
        actions.setBackgroundColor(COLOR_PANEL);

        Button refresh = flatButton("목록 새로고침");
        refresh.setOnClickListener(v -> showLibrary());
        actions.addView(refresh, new LinearLayout.LayoutParams(0, dp(52), 1f));

        Button pick = flatButton("APK/ZIP 가져오기");
        pick.setOnClickListener(v -> openPicker());
        actions.addView(pick, new LinearLayout.LayoutParams(0, dp(52), 1f));

        root.addView(actions);

        applyStatusBarInset(root);
        setContentView(root);
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
            confirmDelete(game);
            return true;
        });

        return row;
    }

    private void confirmDelete(File game) {
        new AlertDialog.Builder(this)
                .setTitle(displayName(game))
                .setMessage("이 게임을 목록에서 삭제할까요?")
                .setNegativeButton("취소", null)
                .setPositiveButton("삭제", (dialog, which) -> {
                    game.delete();
                    showLibrary();
                })
                .show();
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

        gameView = new GameView(this);
        root.addView(gameView, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));

        playerStatus = new TextView(this);
        playerStatus.setText("게임을 시작하는 중...");
        playerStatus.setTextColor(Color.DKGRAY);
        playerStatus.setTextSize(11f);
        playerStatus.setGravity(android.view.Gravity.CENTER);
        playerStatus.setBackgroundColor(Color.WHITE);
        root.addView(playerStatus, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(22)));

        LinearLayout softKeys = new LinearLayout(this);
        softKeys.setBackgroundColor(Color.rgb(202, 202, 202));
        softKeys.addView(keyButton("메뉴", 5), new LinearLayout.LayoutParams(0, dp(42), 1f));
        softKeys.addView(keyButton("뒤로", 6), new LinearLayout.LayoutParams(0, dp(42), 1f));
        root.addView(softKeys);

        root.addView(createPhoneKeypad(), new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(196)));

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

    private View createPhoneKeypad() {
        GridLayout grid = new GridLayout(this);
        grid.setColumnCount(5);
        grid.setRowCount(3);
        grid.setPadding(dp(3), dp(3), dp(3), dp(3));
        grid.setBackgroundColor(Color.rgb(180, 180, 180));

        addKey(grid, "1", 9, 0, 0);
        addKey(grid, "2\nabc", 10, 1, 0);
        addKey(grid, "3\ndef", 11, 2, 0);
        addKey(grid, "*", 18, 3, 0);
        addKey(grid, "← 지움", 7, 4, 0);

        addKey(grid, "4\nghi", 12, 0, 1);
        addKey(grid, "5\njkl", 13, 1, 1);
        addKey(grid, "6\nmno", 14, 2, 1);
        addKey(grid, "0", 8, 3, 1);
        addKey(grid, "↵", 4, 4, 1);

        addKey(grid, "7\npqrs", 15, 0, 2);
        addKey(grid, "8\ntuv", 16, 1, 2);
        addKey(grid, "9\nwxyz", 17, 2, 2);
        addKey(grid, "#", 19, 3, 2);

        grid.addView(new DpadView(this), cellParams(4, 2));

        return grid;
    }

    private void addKey(GridLayout grid, String label, int keyIndex, int column, int row) {
        grid.addView(keyButton(label, keyIndex), cellParams(column, row));
    }

    private GridLayout.LayoutParams cellParams(int column, int row) {
        GridLayout.LayoutParams params = new GridLayout.LayoutParams(GridLayout.spec(row, 1, 1f), GridLayout.spec(column, 1, 1f));
        params.width = 0;
        params.height = 0;
        params.setMargins(dp(2), dp(2), dp(2), dp(2));
        return params;
    }

    private Button keyButton(String label, int keyIndex) {
        Button button = new Button(this);
        button.setText(label);
        button.setTextSize(15f);
        button.setTextColor(Color.BLACK);
        button.setAllCaps(false);
        button.setPadding(0, 0, 0, 0);
        button.setOnTouchListener((view, event) -> handleKeyTouch(view, event, keyIndex));
        return button;
    }

    private boolean handleKeyTouch(View view, MotionEvent event, int keyIndex) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                Log.d(TAG, "key down: " + keyIndex);
                NativeBridge.nativeKey(keyIndex, 1);
                return true;
            case MotionEvent.ACTION_UP:
            case MotionEvent.ACTION_CANCEL:
                Log.d(TAG, "key up: " + keyIndex);
                NativeBridge.nativeKey(keyIndex, 0);
                view.performClick();
                return true;
            default:
                return false;
        }
    }

    // --- helpers ---------------------------------------------------------

    private Button flatButton(String label) {
        Button button = new Button(this);
        button.setText(label);
        button.setTextSize(16f);
        button.setTextColor(Color.WHITE);
        button.setAllCaps(false);
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
     * Four-way pad occupying one keypad cell. Whichever direction the touch
     * leans furthest is held down, so sliding across the pad releases the old
     * direction before pressing the new one.
     */
    private final class DpadView extends View {
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private int active = -1;

        DpadView(MainActivity activity) {
            super(activity);
            paint.setTextAlign(Paint.Align.CENTER);
            paint.setTypeface(Typeface.DEFAULT_BOLD);
        }

        @Override
        protected void onDraw(Canvas canvas) {
            paint.setColor(Color.rgb(70, 70, 70));
            canvas.drawRoundRect(0, 0, getWidth(), getHeight(), dp(4), dp(4), paint);

            paint.setColor(Color.WHITE);
            paint.setTextSize(dp(13));

            float centerX = getWidth() / 2f;
            float centerY = getHeight() / 2f;

            canvas.drawText("▲", centerX, dp(15), paint);
            canvas.drawText("▼", centerX, getHeight() - dp(4), paint);
            canvas.drawText("◀", dp(12), centerY + dp(5), paint);
            canvas.drawText("▶", getWidth() - dp(12), centerY + dp(5), paint);
        }

        @Override
        public boolean onTouchEvent(MotionEvent event) {
            switch (event.getActionMasked()) {
                case MotionEvent.ACTION_DOWN:
                case MotionEvent.ACTION_MOVE:
                    press(direction(event));
                    return true;
                case MotionEvent.ACTION_UP:
                case MotionEvent.ACTION_CANCEL:
                    press(-1);
                    return true;
                default:
                    return true;
            }
        }

        private int direction(MotionEvent event) {
            float offsetX = event.getX() - getWidth() / 2f;
            float offsetY = event.getY() - getHeight() / 2f;

            if (Math.abs(offsetX) > Math.abs(offsetY)) {
                return offsetX < 0 ? 2 : 3;
            }
            return offsetY < 0 ? 0 : 1;
        }

        private void press(int direction) {
            if (direction == active) {
                return;
            }

            if (active >= 0) {
                NativeBridge.nativeKey(active, 0);
            }
            active = direction;
            if (active >= 0) {
                NativeBridge.nativeKey(active, 1);
            }
        }
    }
}
