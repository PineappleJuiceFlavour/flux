package dev.flux;

import net.fabricmc.api.ClientModInitializer;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public class Flux implements ClientModInitializer {
	public static final String MOD_ID = "flux";
	public static final Logger LOGGER = LoggerFactory.getLogger("Flux");

	@Override
	public void onInitializeClient() {
		Benchmark.configureFromSystemProperties();
	}
}
