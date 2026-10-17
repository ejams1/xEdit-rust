{
  Parity #41 probe: a second Free of an object (Report masters.pas frees its
  lists in Finalize, which its Process calls before the run calls it
  again). The memory manager finds the block already free and raises
  EInvalidPointer; the probe pins the class, the message and that the line
  after the second Free does not run.
}
unit FreeTwice;

var
  sl: TStringList;

function Initialize: integer;
begin
  sl := TStringList.Create;
  sl.Add('one');
  sl.Free;
  AddMessage('free | first');
  try
    sl.Free;
    AddMessage('free | second, no error');
  except
    on E: Exception do
      AddMessage('free | second: ' + E.ClassName + ': ' + E.Message);
  end;
  sl := TStringList.Create;
  sl.Free;
  sl.Free;
  AddMessage('free | not reached');
  Result := 0;
end;

end.
