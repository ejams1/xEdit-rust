{ parity probe: x-107 }
unit P_x_107;

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
  v := 1; v := v + 'x';
  AddMessage('after');
end;
end.
