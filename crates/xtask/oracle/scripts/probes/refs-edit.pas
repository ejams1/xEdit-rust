{
  Parity fixture (issue #23): the referenced-by lists and the editor ID
  index before and after edits. The master holds 600 keywords (above
  wbCacheRecordsThreshold, so a reference build writes the cache) and
  activators that refer to some of them. The lists are read, a keyword of
  an activator is replaced, one added and one removed, and the lists are
  read again; then editor IDs are looked up through the group (a search)
  and through the file (the EditorID index, which game settings keep with
  dfIndexEditorID), renamed (one onto a name another record holds:
  Duplicate Key in Index), removed, and a new record is added and removed.
}
unit RefsEdit;

var
  gFile: IInterface;

procedure ListRefs(const aTag: string; aFormID: Cardinal);
var
  rec, r: IInterface;
  i: integer;
  s: string;
begin
  rec := RecordByFormID(gFile, aFormID, False);
  s := '';
  for i := 0 to Pred(ReferencedByCount(rec)) do begin
    r := ReferencedByIndex(rec, i);
    if s <> '' then
      s := s + ', ';
    s := s + Name(r);
  end;
  AddMessage(aTag + ': ' + Name(rec) + ' referenced by ' + IntToStr(ReferencedByCount(rec)) + ' [' + s + ']');
end;

procedure Lookup(const aSignature, aEditorID: string);
var
  rec: IInterface;
begin
  rec := MainRecordByEditorID(GroupBySignature(gFile, aSignature), aEditorID);
  if Assigned(rec) then
    AddMessage('group lookup ' + aEditorID + ' -> ' + Name(rec))
  else
    AddMessage('group lookup ' + aEditorID + ' -> nil');
  rec := RecordByEditorID(gFile, aEditorID);
  if Assigned(rec) then
    AddMessage('index lookup ' + aEditorID + ' -> ' + Name(rec))
  else
    AddMessage('index lookup ' + aEditorID + ' -> nil');
end;

procedure IndexProbe(const aSignature: string; aOne, aTwo, aThree, aFour: Cardinal; const aPrefix: string);
var
  r1, r2, r3, r4, added: IInterface;
begin
  Lookup(aSignature, aPrefix + 'One');
  Lookup(aSignature, LowerCase(aPrefix + 'One'));
  Lookup(aSignature, aPrefix + 'Two');
  r1 := RecordByFormID(gFile, aOne, False);
  r2 := RecordByFormID(gFile, aTwo, False);
  r3 := RecordByFormID(gFile, aThree, False);
  r4 := RecordByFormID(gFile, aFour, False);
  SetElementEditValues(r2, 'EDID', aPrefix + 'Renamed');
  Lookup(aSignature, aPrefix + 'Two');
  Lookup(aSignature, aPrefix + 'Renamed');
  SetElementEditValues(r3, 'EDID', aPrefix + 'Renamed');
  Lookup(aSignature, aPrefix + 'Renamed');
  Lookup(aSignature, aPrefix + 'One');
  SetElementEditValues(r2, 'EDID', aPrefix + 'Two');
  Lookup(aSignature, aPrefix + 'Renamed');
  Lookup(aSignature, aPrefix + 'Two');
  SetElementEditValues(r3, 'EDID', aPrefix + 'One');
  Lookup(aSignature, aPrefix + 'One');
  RemoveElement(r4, 'EDID');
  Lookup(aSignature, aPrefix + 'Four');
  added := Add(GroupBySignature(gFile, aSignature), aSignature, True);
  AddMessage('added ' + Name(added));
  SetElementEditValues(added, 'EDID', aPrefix + 'New');
  Lookup(aSignature, aPrefix + 'New');
  SetElementEditValues(added, 'EDID', aPrefix + 'Two');
  Lookup(aSignature, aPrefix + 'Two');
  Lookup(aSignature, aPrefix + 'New');
  RemoveNode(added);
  Lookup(aSignature, aPrefix + 'Two');
end;

function Initialize: integer;
var
  i: integer;
  f, acti, kwda, el: IInterface;
begin
  Result := 0;
  for i := 0 to Pred(FileCount) do begin
    f := FileByIndex(i);
    if SameText(GetFileName(f), 'Skyrim.esm') then
      gFile := f;
  end;
  ListRefs('initial', $00000800);
  ListRefs('initial', $00000801);
  ListRefs('initial', $00000802);
  ListRefs('initial', $00000803);

  acti := RecordByFormID(gFile, $00001000, False);
  kwda := ElementByPath(acti, 'Keywords\KWDA');
  AddMessage('KWDA of ' + Name(acti) + ': ' + GetEditValue(kwda));
  SetNativeValue(ElementByIndex(kwda, 1), $00000802);
  AddMessage('KWDA after set: ' + GetEditValue(kwda));
  ListRefs('after set', $00000801);
  ListRefs('after set', $00000802);

  el := ElementAssign(kwda, HighInteger, nil, False);
  SetNativeValue(el, $00000803);
  AddMessage('KWDA after add: ' + GetEditValue(kwda));
  ListRefs('after add', $00000803);

  RemoveByIndex(kwda, 0, True);
  AddMessage('KWDA after remove: ' + GetEditValue(kwda));
  ListRefs('after remove', $00000800);
  ListRefs('after remove', $00000802);

  IndexProbe('GMST', $00001800, $00001801, $00001802, $00001803, 'iParity');
  IndexProbe('KYWD', $00000805, $00000806, $00000807, $00000808, 'Kwd');
  AddMessage('done');
end;

end.
