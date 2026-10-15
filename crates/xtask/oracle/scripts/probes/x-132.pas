{ parity probe: x-132 }
unit P_x_132;

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
  AddMessage(Copy(12345, 2, 2));
  AddMessage('after');
end;
end.
