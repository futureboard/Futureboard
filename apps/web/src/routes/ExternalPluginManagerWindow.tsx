import "../App.css";
import { AudioPluginManager } from "../components/plugins/AudioPluginManager";

export function ExternalPluginManagerWindow() {
  return (
    <div className="h-screen w-screen overflow-hidden bg-[#0e1319] text-daw-text">
      <AudioPluginManager windowId="pluginManager" external />
    </div>
  );
}
