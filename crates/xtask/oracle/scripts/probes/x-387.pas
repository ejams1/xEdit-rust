{ parity probe: x-387 }
unit P_x_387;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := TList.Create; v.Add(5); v.Free; v := nil; AddMessage('ok');
  AddMessage('after');
end;
end.
