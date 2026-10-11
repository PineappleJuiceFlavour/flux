package dev.flux;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.mojang.blaze3d.systems.RenderSystem;
import com.mojang.renderpearl.api.device.DeviceInfo;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.LocalDateTime;
import java.time.format.DateTimeFormatter;
import java.util.Arrays;
import java.util.Locale;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.SharedConstants;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.components.debug.DebugEntrySystemSpecs;
import net.minecraft.client.gui.screens.TitleScreen;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.LevelSettings;
import net.minecraft.world.level.WorldDataConfiguration;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.presets.WorldPresets;
import net.minecraft.world.phys.Vec3;

/**
 * A repeatable FPS benchmark: a fixed-seed spectator world, a scripted camera flight and frame-time
 * statistics written to {@code <game dir>/flux/bench-latest.json}.
 *
 * <p>Started by {@code -Dflux.bench=<seconds>} (the launcher's Benchmark button) or {@link #request}.
 * {@code -Dflux.bench.quit=true} closes the game once the result is written.
 */
public final class Benchmark {
	public static final String WORLD_ID = "flux-benchmark";
	private static final long SEED = "flux".hashCode();
	private static final double WARMUP_SECONDS = 12.0;
	private static final double FLIGHT_SPEED = 12.0; // blocks per second
	private static final double FLIGHT_HEIGHT = 24.0; // above the spawn point
	private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();

	private enum State { IDLE, OPENING, LOADING, WARMUP, MEASURE }

	private static State state = State.IDLE;
	private static double measureSeconds;
	private static boolean quitWhenDone;
	private static long phaseStartNanos;
	private static long lastFrameNanos;
	private static Vec3 origin;
	private static float[] frameMillis = new float[1 << 16];
	private static int frameCount;

	private Benchmark() {
	}

	public static void configureFromSystemProperties() {
		String seconds = System.getProperty("flux.bench");
		if (seconds != null) {
			try {
				request(Double.parseDouble(seconds), Boolean.getBoolean("flux.bench.quit"));
			} catch (NumberFormatException e) {
				Flux.LOGGER.error("flux.bench must be a number of seconds, got {}", seconds);
			}
		}
	}

	public static void request(double seconds, boolean quit) {
		measureSeconds = Math.max(5.0, seconds);
		quitWhenDone = quit;
		state = State.OPENING;
		Flux.LOGGER.info("Benchmark requested: {}s flight after {}s warm-up", measureSeconds, WARMUP_SECONDS);
	}

	public static boolean isRunning() {
		return state != State.IDLE;
	}

	public static void onFrame(Minecraft minecraft) {
		if (state == State.IDLE) {
			return;
		}

		long now = System.nanoTime();
		switch (state) {
			case OPENING -> {
				// Wait for the title screen so the game has finished loading resources.
				if (minecraft.level == null && minecraft.gui.screen() instanceof TitleScreen) {
					openWorld(minecraft);
					state = State.LOADING;
				}
			}
			case LOADING -> {
				if (minecraft.level != null && minecraft.player != null && minecraft.gui.screen() == null) {
					// A benchmark must keep rendering even when the window is behind something else.
					minecraft.options.pauseOnLostFocus = false;
					origin = minecraft.player.position().add(0, FLIGHT_HEIGHT, 0);
					phaseStartNanos = now;
					state = State.WARMUP;
					Flux.LOGGER.info("Benchmark world loaded, warming up");
				}
			}
			case WARMUP -> {
				double t = (now - phaseStartNanos) / 1e9;
				// Slow turn on the spot so every direction's chunks are built before measuring.
				placeCamera(minecraft.player, origin, (float) (t / WARMUP_SECONDS * 360.0), 10f);
				if (t >= WARMUP_SECONDS) {
					phaseStartNanos = now;
					lastFrameNanos = now;
					frameCount = 0;
					state = State.MEASURE;
					Flux.LOGGER.info("Benchmark measuring for {}s", measureSeconds);
				}
			}
			case MEASURE -> {
				recordFrame((now - lastFrameNanos) / 1e6f);
				lastFrameNanos = now;
				double t = (now - phaseStartNanos) / 1e9;
				// Fly east (new chunks keep streaming in) while panning left and right.
				Vec3 pos = origin.add(t * FLIGHT_SPEED, 0, 0);
				float yaw = -90f + (float) (Math.sin(t * 0.5) * 70.0);
				float pitch = 12f + (float) (Math.sin(t * 0.23) * 8.0);
				placeCamera(minecraft.player, pos, yaw, pitch);
				if (t >= measureSeconds) {
					finish(minecraft, t);
				}
			}
			default -> {
			}
		}
	}

	private static void openWorld(Minecraft minecraft) {
		if (minecraft.getLevelSource().levelExists(WORLD_ID)) {
			minecraft.createWorldOpenFlows().openWorld(WORLD_ID, () -> minecraft.gui.setScreen(new TitleScreen()));
			return;
		}
		LevelSettings settings = new LevelSettings(
			"Flux Benchmark", GameType.SPECTATOR, LevelSettings.DifficultySettings.DEFAULT, true, WorldDataConfiguration.DEFAULT
		);
		WorldOptions options = new WorldOptions(SEED, true, false);
		minecraft.createWorldOpenFlows().createFreshLevel(WORLD_ID, settings, options, WorldPresets::createNormalWorldDimensions, new TitleScreen());
	}

	private static void placeCamera(LocalPlayer player, Vec3 pos, float yaw, float pitch) {
		if (player == null) {
			return;
		}
		player.snapTo(pos.x, pos.y, pos.z, yaw, pitch);
		player.setDeltaMovement(Vec3.ZERO);
		// No interpolation from the previous tick: the camera is exactly where the script puts it.
		player.setOldPosAndRot();
	}

	private static void recordFrame(float millis) {
		if (frameCount == frameMillis.length) {
			frameMillis = Arrays.copyOf(frameMillis, frameMillis.length * 2);
		}
		frameMillis[frameCount++] = millis;
	}

	private static void finish(Minecraft minecraft, double seconds) {
		state = State.IDLE;
		JsonObject result = summarize(minecraft, seconds);
		Flux.LOGGER.info("Benchmark result: avg {} fps, 1% low {} fps, 0.1% low {} fps over {} frames",
			result.get("avg_fps"), result.get("low_1pct_fps"), result.get("low_0_1pct_fps"), result.get("frames"));

		Path dir = minecraft.gameDirectory.toPath().resolve("flux");
		try {
			Files.createDirectories(dir);
			String json = GSON.toJson(result);
			String stamp = LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyyMMdd-HHmmss"));
			Files.writeString(dir.resolve("bench-" + stamp + ".json"), json, StandardCharsets.UTF_8);
			Files.writeString(dir.resolve("bench-latest.json"), json, StandardCharsets.UTF_8);
		} catch (IOException e) {
			Flux.LOGGER.error("Unable to write benchmark result", e);
		}

		if (quitWhenDone) {
			minecraft.stop();
		}
	}

	private static JsonObject summarize(Minecraft minecraft, double seconds) {
		float[] sorted = Arrays.copyOf(frameMillis, frameCount);
		Arrays.sort(sorted);
		JsonObject result = new JsonObject();
		result.addProperty("frames", frameCount);
		result.addProperty("seconds", round(seconds));
		result.addProperty("avg_fps", round(frameCount / seconds));
		// "1% low" = average FPS of the slowest 1% of frames, as most benchmark tools report it.
		result.addProperty("low_1pct_fps", round(lowFps(sorted, 0.01)));
		result.addProperty("low_0_1pct_fps", round(lowFps(sorted, 0.001)));
		result.addProperty("p50_ms", round(percentile(sorted, 0.50)));
		result.addProperty("p95_ms", round(percentile(sorted, 0.95)));
		result.addProperty("p99_ms", round(percentile(sorted, 0.99)));
		result.addProperty("max_ms", round(frameCount == 0 ? 0 : sorted[frameCount - 1]));

		JsonObject system = new JsonObject();
		system.addProperty("minecraft", SharedConstants.getCurrentVersion().name());
		system.addProperty("java", System.getProperty("java.version"));
		system.addProperty("cpu", DebugEntrySystemSpecs.getCpuInfo());
		try {
			DeviceInfo device = RenderSystem.getDevice().getDeviceInfo();
			system.addProperty("gpu", device.name());
			system.addProperty("backend", device.backendName());
			system.addProperty("driver", device.driverInfo().lines().findFirst().orElse(""));
		} catch (Throwable ignored) {
			// Device info is diagnostic only.
		}
		system.addProperty("window", minecraft.getWindow().getWidth() + "x" + minecraft.getWindow().getHeight());
		system.addProperty("render_distance", minecraft.options.getEffectiveRenderDistance());
		system.addProperty("max_memory_mb", Runtime.getRuntime().maxMemory() / (1024 * 1024));
		result.add("system", system);

		JsonArray mods = new JsonArray();
		FabricLoader.getInstance().getAllMods().stream()
			.map(mod -> mod.getMetadata().getId() + " " + mod.getMetadata().getVersion().getFriendlyString())
			.filter(id -> !id.startsWith("java ") && !id.startsWith("minecraft ") && !id.startsWith("fabric-") && !id.startsWith("mixinextras"))
			.sorted()
			.forEach(mods::add);
		result.add("mods", mods);
		return result;
	}

	private static double lowFps(float[] sorted, double fraction) {
		int n = Math.max(1, (int) Math.ceil(sorted.length * fraction));
		if (sorted.length == 0) {
			return 0;
		}
		double sum = 0;
		for (int i = sorted.length - n; i < sorted.length; i++) {
			sum += sorted[i];
		}
		return 1000.0 / (sum / n);
	}

	private static double percentile(float[] sorted, double p) {
		if (sorted.length == 0) {
			return 0;
		}
		return sorted[Math.min(sorted.length - 1, (int) Math.floor(p * sorted.length))];
	}

	private static double round(double value) {
		return Math.round(value * 10.0) / 10.0;
	}

	static String describe(JsonObject result) {
		return String.format(Locale.ROOT, "%s fps avg, %s fps 1%% low", result.get("avg_fps"), result.get("low_1pct_fps"));
	}
}
