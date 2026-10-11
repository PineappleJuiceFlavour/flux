package dev.flux.mixin;

import dev.flux.Benchmark;
import net.minecraft.client.Minecraft;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(Minecraft.class)
public class MinecraftMixin {
	/** One call per rendered frame, before the frame's ticks and rendering. */
	@Inject(method = "runTick", at = @At("HEAD"))
	private void flux$onFrame(boolean advanceGameTime, CallbackInfo ci) {
		Benchmark.onFrame((Minecraft) (Object) this);
	}
}
