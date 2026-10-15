{ parity probe: x-394 }
unit P_x_394;

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
  v := TList.Create; v.Add(5); v.Add(6); v.Delete(1.5);
  AddMessage('after');
end;
end.
