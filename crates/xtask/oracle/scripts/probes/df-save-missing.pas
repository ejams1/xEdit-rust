{
  Parity fixture (issue #31): the uncaught `SaveToFile` of a data-format
  root into a folder that does not exist.

  Upstream `TdfElement.SaveToFile` creates the file with
  `TFileStream.Create(aFileName, fmCreate)`, which raises `EFOpenError`
  with the `Cannot create file "<name>". <system text>` message; the run
  reports the unhandled exception with its class and the script's unit and
  call line. The probe is deliberately uncaught: the abort line is the
  compared evidence.
}
unit P_df_save_missing;

function Initialize: integer;
var
  doc: TwbDDSFile;
begin
  Result := 0;
  doc := TwbDDSFile.Create;
  doc.SaveToFile('C:\xedit-parity-no-dir\nu\probe.dds');
  AddMessage('after');
end;

end.
