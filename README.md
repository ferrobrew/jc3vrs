# jc3vrs

jc3vrs is a work-in-progress VR mod for Just Cause 3. It renders the game in stereo for an HMD via OpenXR, with a separate floating panel for the UI.

The mod is a DLL injected into the running game. Inside the process, it hooks the engine's rendering, camera, input, and post-processing to drive an OpenXR session: an off-axis per-eye projection into the game's own renderer, a body/head model for on-foot locomotion, and reprojection to the headset.
