Animated GIFs for the "Tip my work" tab.

Drop any .gif files you want to show there into this folder.

HOW IT WORKS

- The GIFs are read at RUNTIME from a "tips" folder next to the executable.
  They are NOT embedded into the binary at compile time. build.rs only embeds
  the application icon and the Windows version metadata.
- If you move or rename the executable, move this folder along with it.
- One GIF is picked at random each time the tab is opened.
- If this folder is missing or empty, the tab works exactly the same, just
  without an animation. The feature is entirely optional.
- Recommended size: 660 px wide or less. Larger GIFs are scaled down, keeping
  their aspect ratio. Only the first 120 frames are decoded, to bound video
  memory usage.

COPYRIGHT

Whatever you put here is yours to account for. Because these files live next
to the executable rather than inside it, they are not distributed with the
released binary — but they WILL be included if you commit them to a public
repository. Use your own material, or material under a licence that permits
redistribution.

The GIFs shipped in the developer's own working copy are not part of the
published release for this reason.
