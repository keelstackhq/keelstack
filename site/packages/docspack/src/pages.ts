/** Pages (file names) that no chunk names as its source in its `<!-- docspack: from X -->` line. */
export function missingPages(pages: readonly string[], chunks: readonly string[]): string[] {
  return pages.filter(
    (page) => !chunks.some((chunk) => chunk.includes(`docspack: from ${page} -->`)),
  );
}
