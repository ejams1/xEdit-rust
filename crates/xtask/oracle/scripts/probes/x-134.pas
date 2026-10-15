{ parity probe: x-134 }
unit P_x_134;

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
  AddMessage(Trim(5));
  AddMessage('after');
end;
end.
