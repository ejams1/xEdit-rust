{ parity probe: x-076 }
unit P_x_076;

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
  v := Chr(65); AddMessage(v);
  AddMessage('after');
end;
end.
