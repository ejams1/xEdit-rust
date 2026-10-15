{ parity probe: x-059 }
unit P_x_059;

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
  v := sl.Nope;
  AddMessage('after');
end;
end.
