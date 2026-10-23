import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen } from "lucide-solid";
import Input from "../Input/Input";
import Button from "../Button/Button";
import useToastState from "@core/states/toastState";

export type FilePathInputProps = {
  value: string,
  onValue: (value: string) => void,
  placeholder?: string,
  dialogTitle?: string,
};

function FilePathInput(props: FilePathInputProps) {
  const { pushError } = useToastState;

  async function browse() {
    try {
      const picked = await open({ multiple: false, directory: false, title: props.dialogTitle });
      if (typeof picked === "string") props.onValue(picked);
    } catch (error) {
      pushError(error);
    }
  }

  return (
    <Input
      value={props.value}
      onValue={props.onValue}
      placeholder={props.placeholder}
      mono
      style={{ "padding-right": "3px" }}
      after={
        <Button icon size="sm" tooltip="Browse" onClick={browse}>
          <FolderOpen size={14} />
        </Button>
      }
    />
  );
}

export default FilePathInput;
