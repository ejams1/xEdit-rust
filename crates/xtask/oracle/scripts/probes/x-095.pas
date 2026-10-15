{ parity probe: x-095 }
unit P_x_095;

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
  v := Unassigned; AddMessage(v);
  AddMessage('after');
end;
end.
