{
  Parity fixture (issue #31): the uncaught `LoadFromFile` of a data-format
  root at a path that does not exist.

  Upstream `TdfElement.LoadFromFile` opens the file with `CreateFile` and
  raises `RaiseLastOSError` when that fails, so the run reports the
  unhandled `EOSError`: its message is `RaiseLastOSError`'s
  (`System Error. Code: 2.` and the system text), its class is `EOSError`,
  and the run line names the script's unit and the line of the call. The
  probe is deliberately uncaught: the abort line is the compared evidence
  (`Exception in unit <unit> line <n>: [EOSError] <message>`).
}
unit P_df_load_missing;

function Initialize: integer;
var
  doc: TwbDDSFile;
begin
  Result := 0;
  doc := TwbDDSFile.Create;
  doc.LoadFromFile('C:\xedit-parity-no-such-file.dds');
  AddMessage('after');
end;

end.
