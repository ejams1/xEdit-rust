{ parity probe: x-109 }
unit P_x_109;

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
  v := 'x'; v := v + '1';
  AddMessage('after');
end;
end.
