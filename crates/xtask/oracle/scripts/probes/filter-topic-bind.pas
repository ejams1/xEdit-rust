{
  Parity #41 probe: the nodes a filter walk binds to the responses of a
  topic when the INOM and INOA fills of an editing session sort the topic's
  group during the walk. Pass 1 of ApplyFilter asks for the conflict status
  of every record it reaches (InheritConflictByParent, the filter of "Apply
  filter for cleaning.pas"); the check of an override topic builds its
  master's topic, whose Init sorts the master's responses (TwbGroupRecord.Sort
  under wbSortINFO) before the walk reaches their nodes, which the
  navigation tree binds as the walk reaches them (vstNavInitNode). The
  Process walk afterwards logs every response the filtered tree holds, in
  the tree's order, so the binding shows (the responses of the override
  file alone, so the message log keeps every line).
}
unit userscript;

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

function Process(e: IInterface): Integer;
begin
  if (Signature(e) = 'INFO') and (GetFileName(GetFile(e)) = 'BrokenSteel.esm') then
    AddMessage(IntToHex(GetLoadOrderFormID(e), 8) + ' ' + GetFileName(GetFile(e)));
  Result := 0;
end;

end.
