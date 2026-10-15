{ parity probe: x-309 }
unit P_x_309;

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
  sl.Move(0, 5);
  AddMessage('after');
end;
end.
