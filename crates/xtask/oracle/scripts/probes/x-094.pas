{ parity probe: x-094 }
unit P_x_094;

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
  v := Null; AddMessage(v);
  AddMessage('after');
end;
end.
