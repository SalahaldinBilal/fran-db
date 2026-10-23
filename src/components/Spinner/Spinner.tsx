import styles from "./Spinner.module.scss";
import { LoaderCircle } from "lucide-solid";

function Spinner(props: { size?: number }) {
  return <LoaderCircle class={styles.Spinner} size={props.size ?? 16} />;
}

export default Spinner;
