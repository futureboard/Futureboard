import "../App.css";
import { ProjectWizard } from "../components/project/ProjectWizard";

export function ExternalProjectWizardWindow() {
  return (
    <div className="h-screen w-screen overflow-hidden bg-[#0e1319] text-daw-text">
      <ProjectWizard windowId="projectWizard" external />
    </div>
  );
}
