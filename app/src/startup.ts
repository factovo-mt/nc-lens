
import { invoke, isDesktop } from "./platform";
import { settings } from "./settings";
import type { PostProfile } from "./types";

interface Launch {
  path: string | null;
  post: PostProfile | null;
  optionalStop: boolean | null;
}

export async function start(
  profile: PostProfile | null,
  showPost: () => void,
  openFile: (path: string) => Promise<void>,
  runNc: () => Promise<void>,
) {
  if (!isDesktop) return runNc();
  let launch: Launch;
  try {
    launch = await invoke<Launch>("startup", {
      post: profile,
      switches: { blockSkip: settings.blockSkip, optionalStop: settings.optionalStop },
    });
  } catch (error) {
    await runNc();
    const state = document.querySelector("#run-state");
    if (state) state.textContent = `起動引数を適用できません：${error}`;
    return;
  }
  if (launch.post) {
    const selected = settings.posts.findIndex((item) => item.profile.name === launch.post!.name);
    const set = { profile: launch.post, path: null };
    if (selected === -1) settings.posts.push(set);
    else settings.posts[selected] = set;
    settings.currentPost = launch.post.name;
    settings.blockSkip = launch.post.expand.block_skip;
    settings.blockSkipLevels = null; // 起動引数のセットの番号に従う
    settings.optionalStop = launch.optionalStop ?? settings.optionalStop;
    showPost();
  }
  if (launch.path) await openFile(launch.path);
  else await runNc();
}
