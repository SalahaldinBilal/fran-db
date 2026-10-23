import { SslFiles } from "@core/types";
import Field, { FieldRow } from "@core/components/Field/Field";
import FilePathInput from "@core/components/FilePathInput/FilePathInput";

export type SslFileFieldsProps = {
  files: SslFiles,
  caHint: string,
  update: (mutate: (files: SslFiles) => void) => void,
};

function SslFileFields(props: SslFileFieldsProps) {
  return (
    <>
      <Field label="CA certificate" hint={props.caHint}>
        <FilePathInput
          value={props.files.caFile}
          onValue={value => props.update(files => { files.caFile = value; })}
          dialogTitle="Choose a CA certificate"
        />
      </Field>
      <FieldRow>
        <Field label="Client certificate" grow>
          <FilePathInput
            value={props.files.certFile}
            onValue={value => props.update(files => { files.certFile = value; })}
            dialogTitle="Choose a client certificate"
          />
        </Field>
        <Field label="Client key" grow>
          <FilePathInput
            value={props.files.keyFile}
            onValue={value => props.update(files => { files.keyFile = value; })}
            dialogTitle="Choose a client key"
          />
        </Field>
      </FieldRow>
    </>
  );
}

export default SslFileFields;
