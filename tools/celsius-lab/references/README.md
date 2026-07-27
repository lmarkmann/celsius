# Scene references

Place a reference photograph here as `<scene>.jpg`, `<scene>.jpeg`, `<scene>.png`, or `<scene>.webp`. Running `just lab-diff <scene>` renders the matching file from root `scenes/`, resizes the reference to the native 104x50 sky buffer, reports RGB and Oklab differences, and writes a scaled heatmap to `out/lab/<scene>_diff.png`.

Use photographs only when their location, time, viewing direction, and weather conditions are known well enough to compare against the scene intent. Keep source and license details beside each image in a matching Markdown file.
