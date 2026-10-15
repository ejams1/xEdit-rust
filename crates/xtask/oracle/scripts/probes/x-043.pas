{ parity probe: x-043 }
unit P_x_043;

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
  s := sl.Strings[-1];
  AddMessage('after');
end;
end.
