{ parity probe: x-214 }
unit P_x_214;

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
  v := 1 + sl;
  AddMessage('after');
end;
end.
