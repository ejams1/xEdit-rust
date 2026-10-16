{
  Parity #35 probe: which responses of Fallout 3's GREETING topic the filter
  for cleaning leaves modified (the AfterLoad trim of wbDialogueTextAfterLoad
  runs as an internal edit when a record's response text is first read).
  The filter runs as "Apply filter for cleaning.pas" applies it; then, for
  each loaded file's version of a few INFO records, the modified and internal
  modified states of the record and of its topic children group, before
  anything else reads them. Only FormIDs of the game master are read: a
  FormID of the plugin raises in the master (LoadOrderFormIDtoFileFormID).
}
unit userscript;

function B(aValue: Boolean): string;
begin
  if aValue then
    Result := 'True'
  else
    Result := 'False';
end;

procedure Report(aFormID: Cardinal);
var
  i: Integer;
  f, r, g: IInterface;
begin
  for i := 0 to Pred(FileCount) do begin
    f := FileByIndex(i);
    r := RecordByFormID(f, LoadOrderFormIDtoFileFormID(f, aFormID), False);
    if not Assigned(r) then
      Continue;
    if not Equals(GetFile(r), f) then
      Continue;
    g := GetContainer(r);
    AddMessage(Format('%s %s modified=%s internal=%s group=%s', [
      GetFileName(f), IntToHex(aFormID, 8),
      B(GetElementState(r, esModified)),
      B(GetElementState(r, esInternalModified)),
      B(GetElementState(g, esModified))
    ]));
  end;
end;

function Initialize: Integer;
begin
  FilterConflictAll := False;
  FilterConflictThis := False;
  FilterByInjectStatus := False;
  FilterInjectStatus := False;
  FilterByNotReachableStatus := False;
  FilterNotReachableStatus := False;
  FilterByReferencesInjectedStatus := False;
  FilterReferencesInjectedStatus := False;
  FilterByEditorID := False;
  FilterEditorID := '';
  FilterByName := False;
  FilterName := '';
  FilterByBaseEditorID := False;
  FilterBaseEditorID := '';
  FilterByBaseName := False;
  FilterBaseName := '';
  FilterScaledActors := False;
  FilterByPersistent := False;
  FilterPersistent := False;
  FilterUnnecessaryPersistent := False;
  FilterMasterIsTemporary := False;
  FilterIsMaster := False;
  FilterPersistentPosChanged := False;
  FilterDeleted := False;
  FilterByVWD := False;
  FilterVWD := False;
  FilterByHasVWDMesh := False;
  FilterHasVWDMesh := False;
  FilterBySignature := False;
  FilterSignatures := '';
  FilterByBaseSignature := False;
  FilterBaseSignatures := '';
  FlattenBlocks := False;
  FlattenCellChilds := False;
  AssignPersWrldChild := False;
  InheritConflictByParent := True;
  ApplyFilter;
  Result := 0;
end;

// The port applies a script's ApplyFilter when the call into the script
// returns (the session is not reachable from inside the call), so the
// states are read in Finalize, after the filter on both sides.
function Finalize: Integer;
begin
  Report($000198DB);
  Report($00025FC9);
  Result := 0;
end;

end.
